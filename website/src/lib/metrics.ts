// Shared formatting for documentation and search. The sync passes its newly
// read published snapshot explicitly, so an older imported module cannot leak
// old numbers into the search index.
import { dateLabel, num, pctLabel } from "./metric-format";

export function expandMetricTokens(source: string, metrics: any): string {
  const values: Record<string, string> = { measured: dateLabel(metrics.measured_at) };
  for (const suite of ["compiler", "conformance"]) {
    const accuracy = metrics.diagnostic_accuracy[suite];
    values["accuracy." + suite + ".recall"] = (accuracy.recall * 100).toFixed(1) + "%";
    values["accuracy." + suite + ".precision"] = (accuracy.precision * 100).toFixed(1) + "%";
    for (const lane of ["js", "errors", "symbols", "types"]) {
      const r = metrics.baselines[suite][lane];
      values[lane + "." + suite + ".percent"] = pctLabel(r.passed, r.passed + r.failed);
      values[lane + "." + suite + ".count"] = num(r.passed) + " / " + num(r.passed + r.failed) + " (" + values[lane + "." + suite + ".percent"] + ")";
    }
  }
  for (const lane of ["js", "declarations"]) {
    const a = metrics.baselines.compiler[lane + "-expanded"];
    const b = metrics.baselines.conformance[lane + "-expanded"];
    values["expanded." + lane + ".percent"] = pctLabel(a.passed + b.passed, a.passed + a.failed + b.passed + b.failed);
  }
  const inventory = metrics.lsp_inventory;
  values["lsp.percent"] = pctLabel(inventory.passed, inventory.passed + inventory.failed);
  values["lsp.skipped"] = num(inventory.skipped);
  values["lsp.table"] = "| Operation | Passed | Failed | Skipped | Pass rate |\n|---|---:|---:|---:|---:|\n" +
    [["quickinfo", "QuickInfo (hover)"], ["completions", "Completions"], ["gotodefinition", "Go-to-definition"],
      ["findallrefs", "Find-all-references"], ["signaturehelp", "Signature help"]].map(([key, label]) => {
      const r = metrics.lsp[key];
      return "| " + label + " | " + num(r.passed) + " | " + num(r.failed) + " | " + num(r.skipped) + " | " + pctLabel(r.passed, r.passed + r.failed) + " |";
    }).join("\n");
  return source.replace(/\{\{metrics\.([^}]+)\}\}/g, (_, key: string) => {
    if (!(key in values)) throw new Error("Unknown metric token: " + key);
    return values[key];
  });
}

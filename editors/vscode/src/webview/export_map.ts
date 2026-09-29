// Webview bundle for the export-map graph view.
// Receives a { nodes, edges } payload from the extension host via postMessage
// and renders a d3-force directed graph into the page's SVG.

import {
  forceCenter,
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
  type Simulation,
  type SimulationLinkDatum,
  type SimulationNodeDatum,
} from "d3-force";
import { drag } from "d3-drag";
import { select, selectAll } from "d3-selection";
import { zoom, zoomIdentity } from "d3-zoom";

interface GraphNode extends SimulationNodeDatum {
  id: string;
  name: string;
  rel?: string;
  dir?: string;
  external: boolean;
  exports?: number;
  edgesIn?: number;
  project?: string;
  inDegree: number;
  outDegree: number;
}

interface GraphEdge extends SimulationLinkDatum<GraphNode> {
  source: string | GraphNode;
  target: string | GraphNode;
  kind: "import" | "reexport" | "reexport-all";
}

interface GraphPayload {
  nodes: Array<Omit<GraphNode, "inDegree" | "outDegree">>;
  edges: GraphEdge[];
}

declare const acquireVsCodeApi: () => {
  postMessage: (msg: unknown) => void;
};

const vscode = acquireVsCodeApi();
let simulation: Simulation<GraphNode, GraphEdge> | undefined;
let currentNodes: GraphNode[] = [];
let currentEdges: GraphEdge[] = [];
let filterText = "";
let showExternal = true;

const root = document.getElementById("graph") as unknown as SVGSVGElement;
const statusEl = document.getElementById("status")!;
const searchEl = document.getElementById("search") as HTMLInputElement;
const toggleExternalEl = document.getElementById("toggle-external") as HTMLInputElement;

const svg = select(root);
const container = svg.append("g").attr("class", "viewport");
const edgeLayer = container.append("g").attr("class", "edges");
const nodeLayer = container.append("g").attr("class", "nodes");

const zoomBehavior = zoom<SVGSVGElement, unknown>()
  .scaleExtent([0.1, 8])
  .on("zoom", (event) => {
    container.attr("transform", event.transform.toString());
  });
svg.call(zoomBehavior as any);

searchEl.addEventListener("input", () => {
  filterText = searchEl.value.trim().toLowerCase();
  applyFilter();
});

toggleExternalEl.addEventListener("change", () => {
  showExternal = toggleExternalEl.checked;
  rebuild();
});

window.addEventListener("message", (event) => {
  const msg = event.data;
  if (msg?.type === "graph") {
    onGraph(msg.payload as GraphPayload);
  }
});

vscode.postMessage({ type: "ready" });

function onGraph(payload: GraphPayload) {
  const degIn = new Map<string, number>();
  const degOut = new Map<string, number>();
  for (const edge of payload.edges) {
    const s = typeof edge.source === "string" ? edge.source : edge.source.id;
    const t = typeof edge.target === "string" ? edge.target : edge.target.id;
    degOut.set(s, (degOut.get(s) ?? 0) + 1);
    degIn.set(t, (degIn.get(t) ?? 0) + 1);
  }
  currentNodes = payload.nodes.map((n) => ({
    ...n,
    inDegree: degIn.get(n.id) ?? 0,
    outDegree: degOut.get(n.id) ?? 0,
  }));
  currentEdges = payload.edges.map((e) => ({ ...e }));
  rebuild();
}

function rebuild() {
  const nodes = currentNodes.filter((n) => showExternal || !n.external);
  const ids = new Set(nodes.map((n) => n.id));
  const edges = currentEdges.filter((e) => {
    const s = typeof e.source === "string" ? e.source : (e.source as GraphNode).id;
    const t = typeof e.target === "string" ? e.target : (e.target as GraphNode).id;
    return ids.has(s) && ids.has(t);
  });

  const internal = nodes.filter((n) => !n.external).length;
  const external = nodes.length - internal;
  statusEl.textContent =
    `${nodes.length} nodes (${internal} files, ${external} external) · ${edges.length} edges`;

  draw(nodes, edges);
}

function nodeRadius(n: GraphNode): number {
  if (n.external) {
    return Math.min(18, 4 + Math.sqrt(n.edgesIn ?? 1) * 1.5);
  }
  const exports = n.exports ?? 0;
  return Math.min(20, 4 + Math.sqrt(exports + 1) * 2.2);
}

function nodeClass(n: GraphNode): string {
  if (n.external) return "node external";
  if ((n.exports ?? 0) === 0) return "node leaf";
  return "node module";
}

function applyFilter() {
  if (!filterText) {
    nodeLayer.selectAll<SVGGElement, GraphNode>(".node").classed("dim", false);
    edgeLayer.selectAll<SVGLineElement, GraphEdge>(".edge").classed("dim", false);
    return;
  }
  nodeLayer
    .selectAll<SVGGElement, GraphNode>(".node")
    .classed("dim", (d) => !matchesFilter(d));
  edgeLayer
    .selectAll<SVGLineElement, GraphEdge>(".edge")
    .classed("dim", (d) => {
      const s = typeof d.source === "string" ? null : (d.source as GraphNode);
      const t = typeof d.target === "string" ? null : (d.target as GraphNode);
      return !(s && matchesFilter(s)) && !(t && matchesFilter(t));
    });
}

function matchesFilter(n: GraphNode): boolean {
  if (!filterText) return true;
  return (
    n.name.toLowerCase().includes(filterText) ||
    (n.rel ?? "").toLowerCase().includes(filterText)
  );
}

function draw(nodes: GraphNode[], edges: GraphEdge[]) {
  if (simulation) simulation.stop();

  simulation = forceSimulation<GraphNode>(nodes)
    .force(
      "link",
      forceLink<GraphNode, GraphEdge>(edges)
        .id((d) => d.id)
        .distance((d) => {
          const s = d.source as GraphNode;
          const t = d.target as GraphNode;
          return s.external || t.external ? 100 : 70;
        })
        .strength(0.5),
    )
    .force("charge", forceManyBody<GraphNode>().strength((d) => (d.external ? -120 : -260)))
    .force("center", forceCenter(root.clientWidth / 2, root.clientHeight / 2))
    .force(
      "collide",
      forceCollide<GraphNode>().radius((d) => nodeRadius(d) + 4),
    )
    .force("x", forceX(root.clientWidth / 2).strength(0.02))
    .force("y", forceY(root.clientHeight / 2).strength(0.02));

  const edgeSel = edgeLayer
    .selectAll<SVGLineElement, GraphEdge>(".edge")
    .data(edges, (d) => `${stringId(d.source)}->${stringId(d.target)}:${d.kind}`)
    .join(
      (enter) =>
        enter
          .append("line")
          .attr("class", (d) => `edge ${d.kind}`)
          .attr("marker-end", "url(#arrow)"),
      (update) => update.attr("class", (d) => `edge ${d.kind}`),
      (exit) => exit.remove(),
    );

  const nodeSel = nodeLayer
    .selectAll<SVGGElement, GraphNode>(".node")
    .data(nodes, (d) => d.id)
    .join(
      (enter) => {
        const g = enter.append("g").attr("class", nodeClass).call(makeDrag());
        g.append("circle").attr("r", nodeRadius);
        g.append("text")
          .attr("dy", (d) => nodeRadius(d) + 11)
          .attr("text-anchor", "middle")
          .text((d) => d.name);
        g.append("title").text((d) => tooltip(d));
        g.on("click", (_event, d) => {
          if (!d.external) {
            vscode.postMessage({ type: "openFile", path: d.id });
          }
        });
        return g;
      },
      (update) => {
        update.attr("class", nodeClass);
        update.select("circle").attr("r", nodeRadius);
        update.select("text").attr("dy", (d) => nodeRadius(d) + 11);
        return update;
      },
      (exit) => exit.remove(),
    );

  simulation.on("tick", () => {
    edgeSel
      .attr("x1", (d) => (d.source as GraphNode).x ?? 0)
      .attr("y1", (d) => (d.source as GraphNode).y ?? 0)
      .attr("x2", (d) => (d.target as GraphNode).x ?? 0)
      .attr("y2", (d) => (d.target as GraphNode).y ?? 0);
    nodeSel.attr("transform", (d) => `translate(${d.x ?? 0},${d.y ?? 0})`);
  });

  applyFilter();
}

function stringId(end: string | GraphNode): string {
  return typeof end === "string" ? end : end.id;
}

function tooltip(n: GraphNode): string {
  if (n.external) {
    return `${n.name}\n${n.edgesIn ?? 0} import(s) from project`;
  }
  return [
    n.rel ?? n.name,
    `exports: ${n.exports ?? 0}`,
    `imports: ${n.outDegree}`,
    `imported by: ${n.inDegree}`,
  ].join("\n");
}

function makeDrag() {
  return drag<SVGGElement, GraphNode>()
    .on("start", (event, d) => {
      if (!event.active) simulation?.alphaTarget(0.3).restart();
      d.fx = d.x;
      d.fy = d.y;
    })
    .on("drag", (event, d) => {
      d.fx = event.x;
      d.fy = event.y;
    })
    .on("end", (event, d) => {
      if (!event.active) simulation?.alphaTarget(0);
      d.fx = null;
      d.fy = null;
    });
}

// Reset zoom on double-click of empty area
svg.on("dblclick.zoom", null);
svg.on("dblclick", (event) => {
  if (event.target === root) {
    (svg as any).call(zoomBehavior.transform, zoomIdentity);
  }
});

// Silence unused import lint
void selectAll;

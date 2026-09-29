import * as esbuild from "esbuild";

const watch = process.argv.includes("--watch");

const extensionOptions = {
  entryPoints: ["src/extension.ts"],
  bundle: true,
  outfile: "dist/extension.js",
  external: ["vscode"],
  format: "cjs",
  legalComments: "none",
  minify: !watch,
  platform: "node",
  sourcemap: true,
  target: "node18",
  logLevel: "info",
};

const webviewOptions = {
  entryPoints: ["src/webview/export_map.ts"],
  bundle: true,
  outfile: "dist/webview/export_map.js",
  format: "iife",
  legalComments: "none",
  minify: !watch,
  platform: "browser",
  sourcemap: true,
  target: "es2020",
  logLevel: "info",
};

if (watch) {
  const ctxExt = await esbuild.context(extensionOptions);
  const ctxView = await esbuild.context(webviewOptions);
  await Promise.all([ctxExt.watch(), ctxView.watch()]);
} else {
  await Promise.all([
    esbuild.build(extensionOptions),
    esbuild.build(webviewOptions),
  ]);
}

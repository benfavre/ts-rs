import React from "react";
import { docsSidebarHtml, docsPagerHtml } from "../../sections/docs";

// Docs shell: sticky sidebar, content column and, on doc pages, an "On this
// page" rail that public/site.js fills from the rendered headings.
export default function DocsLayout(props: { children?: React.ReactNode; route?: { pathname?: string } }) {
  const path = props.route?.pathname || "/docs";
  const isDoc = path !== "/docs" && path.startsWith("/docs/");
  const shell = isDoc ? "d-shell" : "d-shell no-toc";
  return (
    <div className={shell}>
      <aside className="d-side" dangerouslySetInnerHTML={{ __html: docsSidebarHtml(path) }} />
      <div className="d-main">
        {props.children}
        <div dangerouslySetInnerHTML={{ __html: isDoc ? docsPagerHtml(path) : "" }} />
      </div>
      {isDoc ? <nav className="d-toc" aria-label="On this page"></nav> : null}
    </div>
  );
}

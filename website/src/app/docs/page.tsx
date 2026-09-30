import React from "react";
import { docsIndexHtml } from "../../sections/docs";

export default function DocsIndex() {
  return <div dangerouslySetInnerHTML={{ __html: docsIndexHtml() }} />;
}

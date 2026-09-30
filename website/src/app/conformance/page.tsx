import React from "react";
import { conformanceHtml } from "../../sections/conformance";

export default function Conformance() {
  return <div dangerouslySetInnerHTML={{ __html: conformanceHtml() }} />;
}

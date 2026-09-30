import React from "react";
import { playgroundHtml } from "../../sections/playground";

export default function Playground() {
  return (
    <>
      <div dangerouslySetInnerHTML={{ __html: playgroundHtml() }} />
      <script type="module" src="/playground.js?v=2"></script>
    </>
  );
}

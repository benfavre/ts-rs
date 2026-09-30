import React from "react";
import { notFoundHtml } from "../sections/chrome";

// Rendered inside the root layout, with status 404, whenever a loader answers
// 404: the catch-all route below and the docs route for an unknown slug.
export default function NotFound() {
  return <div dangerouslySetInnerHTML={{ __html: notFoundHtml() }} />;
}

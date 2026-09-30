import React from "react";
import { progressHtml } from "../../sections/progress";

export default function Progress() {
  return <div dangerouslySetInnerHTML={{ __html: progressHtml() }} />;
}

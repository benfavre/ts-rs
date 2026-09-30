import React from "react";
import { homeHtml } from "../sections/home";

export default function Home() {
  return <div dangerouslySetInnerHTML={{ __html: homeHtml() }} />;
}

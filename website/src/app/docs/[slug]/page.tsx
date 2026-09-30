import React from "react";
import { readmeStatus } from "../../../lib/repo.generated";
import { renderDoc } from "../../../lib/markdown";
import { docNotFoundHtml, docSourceHtml } from "../../../sections/docs";

declare function __readFile(path: string): string;
declare function __readFileExists(path: string): boolean;

interface SlugProps {
  params?: { slug?: string[] | string };
}

function contentRoot(): string {
  return "/home/infra-sj278/bext/sites/ts-rs-prism/content/docs";
}

function docFile(s: string[] | string | undefined): { slug: string; file: string } | null {
  const raw = Array.isArray(s) ? s.join("/") : (s ?? "");
  const slug = raw.replace(/[^a-z0-9/-]/g, "");
  const file = contentRoot() + "/" + slug + ".md";
  if (!slug || slug !== raw || !__readFileExists(file)) return null;
  return { slug: slug, file: file };
}

// An unknown slug answers 404, which renders app/not-found.tsx in the layout.
export async function loader(args: { request?: { url?: string }; params?: { slug?: string[] | string } }): Promise<any> {
  // Prefer the URL: it is present whatever shape the loader params take.
  const url = String(args.request?.url || "");
  const m = /\/docs\/([^?#/]*)/.exec(url);
  const fromUrl = m ? m[1].replace(/\/+$/, "") : "";
  if (!docFile(fromUrl || args.params?.slug)) return new Response(null, { status: 404 });
  return {};
}

// Reads content/docs/<slug>.md at render time, so a new or edited Markdown
// file is live without a rebuild.
export default function DocPage(props: SlugProps) {
  const found = docFile(props.params?.slug);
  if (!found) {
    return <div dangerouslySetInnerHTML={{ __html: docNotFoundHtml("") }} />;
  }
  const slug = found.slug;
  const file = found.file;
  const html = renderDoc(__readFile(file), readmeStatus().metrics) + docSourceHtml(slug);
  return <article className="doc" dangerouslySetInnerHTML={{ __html: html }} />;
}

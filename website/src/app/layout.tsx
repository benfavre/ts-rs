import React from "react";
import { repoSource } from "../lib/repo.generated";
import { SITE } from "../lib/site";
import { THEME_CSS } from "../lib/theme";
import { fontFaceCss, FONT_PRELOAD_SANS, FONT_PRELOAD_MONO } from "../lib/fonts";
import { extraCss } from "../lib/theme-extra";
import { navInnerHtml, footerInnerHtml, pageMeta, ogImage, jsonLd, headScript } from "../sections/chrome";

// Root document for ts-rs.bext.dev. Navigation, footer and page sections are
// built as HTML strings (see lib/html.ts); this file only wires them together.
export default function RootLayout(props: { children?: React.ReactNode; route?: { pathname?: string } }) {
  const path = props.route?.pathname || "/";
  const meta = pageMeta(path);
  const url = SITE + (path === "/" ? "/" : path);
  const css = fontFaceCss() + THEME_CSS + extraCss();
  return (
    <html lang="en" data-build={repoSource().commit}>
      <head>
        <meta charSet="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <title>{meta.title}</title>
        <meta name="description" content={meta.desc} />
        <meta name="theme-color" content="#faf9f7" media="(prefers-color-scheme: light)" />
        <meta name="theme-color" content="#0e1014" media="(prefers-color-scheme: dark)" />
        <meta name="color-scheme" content="light dark" />
        <link rel="icon" type="image/svg+xml" href="/favicon.svg" />
        <link rel="canonical" href={url} />
        <link rel="alternate" type="application/atom+xml" title="tsc-rs verified waves" href="/feed.xml" />
        <meta property="og:type" content="website" />
        <meta property="og:site_name" content="tsc-rs" />
        <meta property="og:title" content={meta.title} />
        <meta property="og:description" content={meta.desc} />
        <meta property="og:url" content={url} />
        <meta property="og:image" content={ogImage(meta)} />
        <meta name="twitter:card" content="summary_large_image" />
        <link rel="preload" as="font" type="font/woff2" href={FONT_PRELOAD_SANS} crossOrigin="" />
        <link rel="preload" as="font" type="font/woff2" href={FONT_PRELOAD_MONO} crossOrigin="" />
        <script dangerouslySetInnerHTML={{ __html: headScript() }} />
        <style dangerouslySetInnerHTML={{ __html: css }} />
        <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: jsonLd() }} />
      </head>
      <body>
        <a className="t-skip" href="#main">Skip to content</a>
        <nav className="t-nav" aria-label="Main" dangerouslySetInnerHTML={{ __html: navInnerHtml(path) }} />
        <main id="main">{props.children}</main>
        <footer className="t-foot" dangerouslySetInnerHTML={{ __html: footerInnerHtml() }} />
        <script src="/site.js?v=4" defer></script>
      </body>
    </html>
  );
}

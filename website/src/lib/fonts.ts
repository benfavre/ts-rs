// Self-hosted webfonts, declared in an inline <style>.
//
// The fleet CSP is style-src 'self' 'unsafe-inline', which refuses the Google
// Fonts stylesheet, so both families are served from /fonts on this origin.
// Inter and JetBrains Mono are variable fonts: one file per unicode subset
// covers every weight.
const LATIN =
  "U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+0304, U+0308, U+0329, U+2000-206F, U+20AC, U+2122, U+2191, U+2193, U+2212, U+2215, U+FEFF, U+FFFD";
const LATIN_EXT =
  "U+0100-02BA, U+02BD-02C5, U+02C7-02CC, U+02CE-02D7, U+02DD-02FF, U+0304, U+0308, U+0329, U+1D00-1DBF, U+1E00-1E9F, U+1EF2-1EFF, U+2020, U+20A0-20AB, U+20AD-20C0, U+2113, U+2C60-2C7F, U+A720-A7FF";

export const FONT_PRELOAD_SANS = "/fonts/inter-latin.woff2";
export const FONT_PRELOAD_MONO = "/fonts/jetbrains-mono-latin.woff2";

export function fontFaceCss(): string {
  return (
    "@font-face{font-family:'Inter';font-style:normal;font-weight:400 900;font-display:swap;src:url(/fonts/inter-latin.woff2) format('woff2');unicode-range:" + LATIN + "}" +
    "@font-face{font-family:'Inter';font-style:normal;font-weight:400 900;font-display:swap;src:url(/fonts/inter-latin-ext.woff2) format('woff2');unicode-range:" + LATIN_EXT + "}" +
    "@font-face{font-family:'JetBrains Mono';font-style:normal;font-weight:100 900;font-display:swap;src:url(/fonts/jetbrains-mono-latin.woff2) format('woff2');unicode-range:" + LATIN + "}"
  );
}

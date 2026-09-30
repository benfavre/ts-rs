// PRISM supplies the JSX runtime. Its HTML props follow React's JSX types;
// provide the namespace for TypeScript without adding a browser React runtime.
declare module "@bext-stack/framework/jsx-runtime" {
  export { JSX } from "react/jsx-runtime";
}

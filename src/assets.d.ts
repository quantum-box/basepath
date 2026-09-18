/**
 * Vite resolves these imports; TypeScript needs to be told they exist.
 * `?raw` is used by the browser harness to load the committed MCP App bundle.
 */
declare module "*.css";
declare module "*.html?raw" {
  const content: string;
  export default content;
}

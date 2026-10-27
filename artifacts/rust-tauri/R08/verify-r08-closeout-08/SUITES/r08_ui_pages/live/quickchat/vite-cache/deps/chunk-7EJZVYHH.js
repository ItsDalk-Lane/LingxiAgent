import {
  getConfig2
} from "./chunk-SKZ5AL3X.js";
import {
  select_default
} from "./chunk-B534XTKK.js";
import {
  __name
} from "./chunk-QPVWWBLP.js";

// node_modules/mermaid/dist/chunks/mermaid.core/chunk-3NCLNEKW.mjs
var selectSvgElement = __name((id) => {
  const { securityLevel } = getConfig2();
  let root = select_default("body");
  if (securityLevel === "sandbox") {
    const sandboxElement = select_default(`#i${id}`);
    const doc = sandboxElement.node()?.contentDocument ?? document;
    root = select_default(doc.body);
  }
  const svg = root.select(`#${id}`);
  return svg;
}, "selectSvgElement");

export {
  selectSvgElement
};
//# sourceMappingURL=chunk-7EJZVYHH.js.map

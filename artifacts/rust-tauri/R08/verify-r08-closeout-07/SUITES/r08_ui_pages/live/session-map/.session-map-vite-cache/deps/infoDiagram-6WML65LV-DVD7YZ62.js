import {
  parse
} from "./chunk-LMNLY7NZ.js";
import "./chunk-WG6RVDYJ.js";
import "./chunk-7KDD2263.js";
import "./chunk-4V37WN4K.js";
import "./chunk-UWVENJQL.js";
import "./chunk-PE2SENRT.js";
import "./chunk-5H72DHF4.js";
import "./chunk-V5ZFIU6P.js";
import "./chunk-DRSPDASF.js";
import "./chunk-2JQVQVIG.js";
import "./chunk-EIVKEYC3.js";
import "./chunk-U7C33TSG.js";
import "./chunk-KI6FIV5J.js";
import "./chunk-DNVQOESM.js";
import "./chunk-TS5F3ZJY.js";
import "./chunk-ZC2KZAT4.js";
import "./chunk-DUGC7XZH.js";
import {
  selectSvgElement
} from "./chunk-6OIFLBNW.js";
import {
  configureSvgSize
} from "./chunk-NNU6PD54.js";
import {
  log
} from "./chunk-GBRQLC4M.js";
import {
  __name
} from "./chunk-F6ZDDTKC.js";
import "./chunk-PR4QN5HX.js";

// node_modules/mermaid/dist/chunks/mermaid.core/infoDiagram-6WML65LV.mjs
var parser = {
  parse: __name(async (input) => {
    const ast = await parse("info", input);
    log.debug(ast);
  }, "parse")
};
var DEFAULT_INFO_DB = {
  version: "11.16.1" + (true ? "" : "-tiny")
};
var getVersion = __name(() => DEFAULT_INFO_DB.version, "getVersion");
var db = {
  getVersion
};
var draw = __name((text, id, version) => {
  log.debug("rendering info diagram\n" + text);
  const svg = selectSvgElement(id);
  configureSvgSize(svg, 100, 400, true);
  const group = svg.append("g");
  group.append("text").attr("x", 100).attr("y", 40).attr("class", "version").attr("font-size", 32).style("text-anchor", "middle").text(`v${version}`);
}, "draw");
var renderer = { draw };
var diagram = {
  parser,
  db,
  renderer
};
export {
  diagram
};
//# sourceMappingURL=infoDiagram-6WML65LV-DVD7YZ62.js.map

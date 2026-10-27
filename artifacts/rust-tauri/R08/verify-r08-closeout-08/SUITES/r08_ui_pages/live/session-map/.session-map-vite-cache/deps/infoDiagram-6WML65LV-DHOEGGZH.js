import {
  parse
} from "./chunk-VSFHZMFE.js";
import "./chunk-XU4QP652.js";
import "./chunk-WGVKMIGS.js";
import "./chunk-VOQXT7KT.js";
import "./chunk-M4ZIGJF3.js";
import "./chunk-3HT46VZ6.js";
import "./chunk-GYA3H4JL.js";
import "./chunk-V2H5KKBA.js";
import "./chunk-T6QIBFSO.js";
import "./chunk-ZYR7UTIC.js";
import "./chunk-DHEW67N4.js";
import "./chunk-KOBNXZJO.js";
import "./chunk-UF6STQ5B.js";
import "./chunk-DFRQR5N2.js";
import "./chunk-ZGHMTEBU.js";
import "./chunk-ZYRYFMDS.js";
import "./chunk-RY4WWWT5.js";
import {
  selectSvgElement
} from "./chunk-7EJZVYHH.js";
import {
  configureSvgSize
} from "./chunk-SKZ5AL3X.js";
import {
  log
} from "./chunk-B534XTKK.js";
import {
  __name
} from "./chunk-QPVWWBLP.js";
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
//# sourceMappingURL=infoDiagram-6WML65LV-DHOEGGZH.js.map

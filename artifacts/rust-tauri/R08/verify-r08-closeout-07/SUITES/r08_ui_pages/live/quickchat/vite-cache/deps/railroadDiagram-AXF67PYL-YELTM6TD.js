import {
  db,
  getStyles,
  renderer
} from "./chunk-2Z7DHRIS.js";
import {
  populateCommonDb
} from "./chunk-GAX5MHEW.js";
import {
  MermaidParseError
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
import {
  createRailroadServices
} from "./chunk-TS5F3ZJY.js";
import "./chunk-ZC2KZAT4.js";
import "./chunk-DUGC7XZH.js";
import "./chunk-6OIFLBNW.js";
import "./chunk-NNU6PD54.js";
import {
  log
} from "./chunk-GBRQLC4M.js";
import {
  __name
} from "./chunk-F6ZDDTKC.js";
import "./chunk-PR4QN5HX.js";

// node_modules/mermaid/dist/chunks/mermaid.core/railroadDiagram-AXF67PYL.mjs
var langiumParser = createRailroadServices().Railroad.parser.LangiumParser;
var transformExpression = __name((expr) => {
  switch (expr.$type) {
    case "RailroadTerminalExpr":
      return {
        type: "terminal",
        value: expr.value
      };
    case "RailroadNonTerminalExpr":
      return {
        type: "nonterminal",
        name: expr.name
      };
    case "RailroadSpecialExpr":
      return {
        type: "special",
        text: expr.text
      };
    case "RailroadSequenceExpr": {
      const elements = expr.elements.map(transformExpression);
      return elements.length === 1 ? elements[0] : { type: "sequence", elements };
    }
    case "RailroadChoiceExpr": {
      const alternatives = expr.alternatives.map(transformExpression);
      return alternatives.length === 1 ? alternatives[0] : { type: "choice", alternatives };
    }
    case "RailroadOptionalExpr":
      return {
        type: "optional",
        element: transformExpression(expr.element)
      };
    case "RailroadOneOrMoreExpr":
      return {
        type: "repetition",
        element: transformExpression(expr.element),
        min: 1,
        max: Infinity
      };
    case "RailroadZeroOrMoreExpr":
      return {
        type: "repetition",
        element: transformExpression(expr.element),
        min: 0,
        max: Infinity
      };
    default:
      throw new Error(`Unsupported railroad expression: ${expr.$type}`);
  }
}, "transformExpression");
var transformRule = __name((rule) => {
  return {
    name: rule.name,
    definition: transformExpression(rule.definition)
  };
}, "transformRule");
var populateDb = __name((ast) => {
  populateCommonDb(ast, db);
  if (ast.title) {
    db.setTitle(ast.title);
  }
  ast.rules.map((rule) => db.addRule(transformRule(rule)));
}, "populateDb");
var parser = {
  parse: __name((input) => {
    db.clear();
    log.debug("[Railroad Parser] Starting Langium parse");
    const result = langiumParser.parse(input);
    if (result.lexerErrors.length > 0 || result.parserErrors.length > 0) {
      throw new MermaidParseError(result);
    }
    const ast = result.value;
    log.debug("[Railroad Parser] Parsed rules:", ast.rules.length);
    populateDb(ast);
    log.debug("[Railroad Parser] Parse complete");
  }, "parse"),
  parser: {
    yy: db
  }
};
var diagram = {
  parser,
  db,
  renderer,
  styles: getStyles
};
var railroadDiagram_default = diagram;
export {
  railroadDiagram_default as default,
  diagram
};
//# sourceMappingURL=railroadDiagram-AXF67PYL-YELTM6TD.js.map

import {
  db,
  getStyles,
  renderer
} from "./chunk-36TMQ7HO.js";
import {
  populateCommonDb
} from "./chunk-CP6ODN5K.js";
import {
  MermaidParseError
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
import {
  createRailroadServices
} from "./chunk-ZGHMTEBU.js";
import "./chunk-ZYRYFMDS.js";
import "./chunk-RY4WWWT5.js";
import "./chunk-7EJZVYHH.js";
import "./chunk-SKZ5AL3X.js";
import {
  log
} from "./chunk-B534XTKK.js";
import {
  __name
} from "./chunk-QPVWWBLP.js";
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
//# sourceMappingURL=railroadDiagram-AXF67PYL-KBG3DRSV.js.map

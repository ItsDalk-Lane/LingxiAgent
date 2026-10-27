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
import {
  createRailroadAbnfServices
} from "./chunk-WGVKMIGS.js";
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
import "./chunk-7EJZVYHH.js";
import "./chunk-SKZ5AL3X.js";
import {
  log
} from "./chunk-B534XTKK.js";
import {
  __name
} from "./chunk-QPVWWBLP.js";
import "./chunk-PR4QN5HX.js";

// node_modules/mermaid/dist/chunks/mermaid.core/abnfDiagram-N423BO3Z.mjs
var langiumParser = createRailroadAbnfServices().RailroadAbnf.parser.LangiumParser;
var transformAlternation = __name((alt) => {
  const alternatives = alt.alternatives.map(transformConcatenation);
  if (alternatives.length === 1) {
    return alternatives[0];
  }
  return {
    type: "choice",
    alternatives
  };
}, "transformAlternation");
var transformConcatenation = __name((concat) => {
  const elements = concat.elements.map(transformElement);
  if (elements.length === 1) {
    return elements[0];
  }
  return {
    type: "sequence",
    elements
  };
}, "transformConcatenation");
var parseRepeat = __name((repeat) => {
  if (repeat.includes("*")) {
    const [minStr, maxStr] = repeat.split("*");
    const min = minStr ? parseInt(minStr, 10) : 0;
    const max = maxStr ? parseInt(maxStr, 10) : Infinity;
    return { min, max };
  }
  const exact = parseInt(repeat, 10);
  return { min: exact, max: exact };
}, "parseRepeat");
var transformElement = __name((element) => {
  const inner = transformPrimary(element.primary);
  if (!element.repeat) {
    return inner;
  }
  const { min, max } = parseRepeat(element.repeat);
  if (min === 0 && max === 1) {
    return { type: "optional", element: inner };
  }
  return {
    type: "repetition",
    element: inner,
    min,
    max
  };
}, "transformElement");
var transformPrimary = __name((primary) => {
  switch (primary.$type) {
    case "AbnfStringLiteral":
      return {
        type: "terminal",
        value: primary.value
      };
    case "AbnfNumVal":
      return {
        type: "terminal",
        value: primary.value
      };
    case "AbnfRuleName":
      return {
        type: "nonterminal",
        name: primary.name
      };
    case "AbnfGroup":
      return transformAlternation(primary.element);
    case "AbnfOptionalGroup":
      return {
        type: "optional",
        element: transformAlternation(primary.element)
      };
    default:
      throw new Error(`Unsupported ABNF primary node: ${primary.$type}`);
  }
}, "transformPrimary");
var transformRule = __name((rule) => {
  return {
    name: rule.name,
    definition: transformAlternation(rule.definition)
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
    log.debug("[ABNF Parser] Starting Langium parse");
    const result = langiumParser.parse(input);
    if (result.lexerErrors.length > 0 || result.parserErrors.length > 0) {
      throw new MermaidParseError(result);
    }
    const ast = result.value;
    log.debug("[ABNF Parser] Parsed rules:", ast.rules.length);
    populateDb(ast);
    log.debug("[ABNF Parser] Parse complete");
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
export {
  diagram
};
//# sourceMappingURL=abnfDiagram-N423BO3Z-6YZGBKX7.js.map

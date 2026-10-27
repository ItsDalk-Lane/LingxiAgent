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
import {
  createRailroadEbnfServices
} from "./chunk-WG6RVDYJ.js";
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
import "./chunk-6OIFLBNW.js";
import "./chunk-NNU6PD54.js";
import {
  log
} from "./chunk-GBRQLC4M.js";
import {
  __name
} from "./chunk-F6ZDDTKC.js";
import "./chunk-PR4QN5HX.js";

// node_modules/mermaid/dist/chunks/mermaid.core/ebnfDiagram-BXEA7PRR.mjs
var langiumParser = createRailroadEbnfServices().RailroadEbnf.parser.LangiumParser;
var transformChoice = __name((choice) => {
  const alternatives = choice.alternatives.map(transformSequence);
  if (alternatives.length === 1) {
    return alternatives[0];
  }
  return {
    type: "choice",
    alternatives
  };
}, "transformChoice");
var transformSequence = __name((sequence) => {
  const elements = sequence.elements.map(transformTerm);
  if (elements.length === 1) {
    return elements[0];
  }
  return {
    type: "sequence",
    elements
  };
}, "transformSequence");
var transformPrimary = __name((primary) => {
  switch (primary.$type) {
    case "EbnfTerminal":
      return {
        type: "terminal",
        value: primary.value
      };
    case "EbnfNonTerminal":
      return {
        type: "nonterminal",
        name: primary.name
      };
    case "EbnfSpecial":
      return {
        type: "special",
        text: primary.text
      };
    case "EbnfGroup":
      return transformChoice(primary.element);
    case "EbnfOptional":
      return {
        type: "optional",
        element: transformChoice(primary.element)
      };
    case "EbnfRepetition":
      return {
        type: "repetition",
        element: transformChoice(primary.element),
        min: 0,
        max: Infinity
      };
    default:
      throw new Error(`Unsupported EBNF primary node: ${primary.$type}`);
  }
}, "transformPrimary");
var transformPostfix = __name((node, postfix) => {
  switch (postfix.$type) {
    case "EbnfOptionalPostfix":
      return {
        type: "optional",
        element: node
      };
    case "EbnfZeroOrMorePostfix":
      return {
        type: "repetition",
        element: node,
        min: 0,
        max: Infinity
      };
    case "EbnfOneOrMorePostfix":
      return {
        type: "repetition",
        element: node,
        min: 1,
        max: Infinity
      };
    case "EbnfExceptionPostfix":
      return {
        type: "sequence",
        elements: [
          node,
          { type: "terminal", value: "-" },
          transformPrimary(postfix.except)
        ]
      };
    default:
      throw new Error(`Unsupported EBNF postfix node: ${postfix.$type}`);
  }
}, "transformPostfix");
var transformTerm = __name((term) => {
  return term.postfixes.reduce((currentNode, postfix) => {
    return transformPostfix(currentNode, postfix);
  }, transformPrimary(term.base));
}, "transformTerm");
var transformRule = __name((rule) => {
  return {
    name: rule.name,
    definition: transformChoice(rule.definition)
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
    log.debug("[EBNF Parser] Starting Langium parse");
    const result = langiumParser.parse(input);
    if (result.lexerErrors.length > 0 || result.parserErrors.length > 0) {
      throw new MermaidParseError(result);
    }
    const ast = result.value;
    log.debug("[EBNF Parser] Parsed rules:", ast.rules.length);
    populateDb(ast);
    log.debug("[EBNF Parser] Parse complete");
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
//# sourceMappingURL=ebnfDiagram-BXEA7PRR-2HKSHWX6.js.map

import {
  LanguageDescription,
  LanguageSupport,
  StreamLanguage
} from "./chunk-2W33IE7H.js";
import "./chunk-CD253LJV.js";
import "./chunk-FCMPVDPN.js";
import "./chunk-G5WJM6S6.js";
import "./chunk-H67WQRR3.js";
import "./chunk-PR4QN5HX.js";

// node_modules/@codemirror/language-data/dist/index.js
function legacy(parser) {
  return new LanguageSupport(StreamLanguage.define(parser));
}
function sql(dialectName) {
  return import("./dist-FZP7VXGZ.js").then((m) => m.sql({ dialect: m[dialectName] }));
}
var languages = [
  // New-style language modes
  LanguageDescription.of({
    name: "C",
    extensions: ["c", "h", "ino"],
    load() {
      return import("./dist-ZUBQUFUP.js").then((m) => m.cpp());
    }
  }),
  LanguageDescription.of({
    name: "C++",
    alias: ["cpp"],
    extensions: ["cpp", "c++", "cc", "cxx", "hpp", "h++", "hh", "hxx"],
    load() {
      return import("./dist-ZUBQUFUP.js").then((m) => m.cpp());
    }
  }),
  LanguageDescription.of({
    name: "CQL",
    alias: ["cassandra"],
    extensions: ["cql"],
    load() {
      return sql("Cassandra");
    }
  }),
  LanguageDescription.of({
    name: "CSS",
    extensions: ["css"],
    load() {
      return import("./dist-XCKDLI22.js").then((m) => m.css());
    }
  }),
  LanguageDescription.of({
    name: "Go",
    extensions: ["go"],
    load() {
      return import("./dist-QLPSNKEV.js").then((m) => m.go());
    }
  }),
  LanguageDescription.of({
    name: "HTML",
    alias: ["xhtml"],
    extensions: ["html", "htm", "handlebars", "hbs"],
    load() {
      return import("./dist-OJOTGQUW.js").then((m) => m.html());
    }
  }),
  LanguageDescription.of({
    name: "Java",
    extensions: ["java"],
    load() {
      return import("./dist-AWNUL2DG.js").then((m) => m.java());
    }
  }),
  LanguageDescription.of({
    name: "JavaScript",
    alias: ["ecmascript", "js", "node"],
    extensions: ["js", "mjs", "cjs"],
    load() {
      return import("./dist-YM7T7Z6L.js").then((m) => m.javascript());
    }
  }),
  LanguageDescription.of({
    name: "Jinja",
    extensions: ["j2", "jinja", "jinja2"],
    load() {
      return import("./dist-THIR7SAK.js").then((m) => m.jinja());
    }
  }),
  LanguageDescription.of({
    name: "JSON",
    alias: ["json5"],
    extensions: ["json", "map"],
    load() {
      return import("./dist-BXNCYFNE.js").then((m) => m.json());
    }
  }),
  LanguageDescription.of({
    name: "JSX",
    extensions: ["jsx"],
    load() {
      return import("./dist-YM7T7Z6L.js").then((m) => m.javascript({ jsx: true }));
    }
  }),
  LanguageDescription.of({
    name: "LESS",
    extensions: ["less"],
    load() {
      return import("./dist-S2RAWQFD.js").then((m) => m.less());
    }
  }),
  LanguageDescription.of({
    name: "Liquid",
    extensions: ["liquid"],
    load() {
      return import("./dist-VEFMF3V4.js").then((m) => m.liquid());
    }
  }),
  LanguageDescription.of({
    name: "MariaDB SQL",
    load() {
      return sql("MariaSQL");
    }
  }),
  LanguageDescription.of({
    name: "Markdown",
    extensions: ["md", "markdown", "mkd"],
    load() {
      return import("./@codemirror_lang-markdown.js").then((m) => m.markdown());
    }
  }),
  LanguageDescription.of({
    name: "MS SQL",
    load() {
      return sql("MSSQL");
    }
  }),
  LanguageDescription.of({
    name: "MySQL",
    load() {
      return sql("MySQL");
    }
  }),
  LanguageDescription.of({
    name: "PHP",
    extensions: ["php", "php3", "php4", "php5", "php7", "phtml"],
    load() {
      return import("./dist-67XH76FR.js").then((m) => m.php());
    }
  }),
  LanguageDescription.of({
    name: "PLSQL",
    extensions: ["pls"],
    load() {
      return sql("PLSQL");
    }
  }),
  LanguageDescription.of({
    name: "PostgreSQL",
    load() {
      return sql("PostgreSQL");
    }
  }),
  LanguageDescription.of({
    name: "Python",
    extensions: ["BUILD", "bzl", "py", "pyw"],
    filename: /^(BUCK|BUILD)$/,
    load() {
      return import("./dist-UT34Q63G.js").then((m) => m.python());
    }
  }),
  LanguageDescription.of({
    name: "Rust",
    extensions: ["rs"],
    load() {
      return import("./dist-NNFTTQRQ.js").then((m) => m.rust());
    }
  }),
  LanguageDescription.of({
    name: "Sass",
    extensions: ["sass"],
    load() {
      return import("./dist-NTKJKCOF.js").then((m) => m.sass({ indented: true }));
    }
  }),
  LanguageDescription.of({
    name: "SCSS",
    extensions: ["scss"],
    load() {
      return import("./dist-NTKJKCOF.js").then((m) => m.sass());
    }
  }),
  LanguageDescription.of({
    name: "SQL",
    extensions: ["sql"],
    load() {
      return sql("StandardSQL");
    }
  }),
  LanguageDescription.of({
    name: "SQLite",
    load() {
      return sql("SQLite");
    }
  }),
  LanguageDescription.of({
    name: "TSX",
    extensions: ["tsx"],
    load() {
      return import("./dist-YM7T7Z6L.js").then((m) => m.javascript({ jsx: true, typescript: true }));
    }
  }),
  LanguageDescription.of({
    name: "TypeScript",
    alias: ["ts"],
    extensions: ["ts", "mts", "cts"],
    load() {
      return import("./dist-YM7T7Z6L.js").then((m) => m.javascript({ typescript: true }));
    }
  }),
  LanguageDescription.of({
    name: "WebAssembly",
    extensions: ["wat", "wast"],
    load() {
      return import("./dist-YTIWYFAI.js").then((m) => m.wast());
    }
  }),
  LanguageDescription.of({
    name: "XML",
    alias: ["rss", "wsdl", "xsd"],
    extensions: ["xml", "xsl", "xsd", "svg"],
    load() {
      return import("./dist-3FS4KLWA.js").then((m) => m.xml());
    }
  }),
  LanguageDescription.of({
    name: "YAML",
    alias: ["yml"],
    extensions: ["yaml", "yml"],
    load() {
      return import("./dist-VWI5TKQA.js").then((m) => m.yaml());
    }
  }),
  // Legacy modes ported from CodeMirror 5
  LanguageDescription.of({
    name: "APL",
    extensions: ["dyalog", "apl"],
    load() {
      return import("./apl-BIJA5MVL.js").then((m) => legacy(m.apl));
    }
  }),
  LanguageDescription.of({
    name: "PGP",
    alias: ["asciiarmor"],
    extensions: ["asc", "pgp", "sig"],
    load() {
      return import("./asciiarmor-XQR436P5.js").then((m) => legacy(m.asciiArmor));
    }
  }),
  LanguageDescription.of({
    name: "ASN.1",
    extensions: ["asn", "asn1"],
    load() {
      return import("./asn1-CUSEGGTN.js").then((m) => legacy(m.asn1({})));
    }
  }),
  LanguageDescription.of({
    name: "Asterisk",
    filename: /^extensions\.conf$/i,
    load() {
      return import("./asterisk-KP5U5XF6.js").then((m) => legacy(m.asterisk));
    }
  }),
  LanguageDescription.of({
    name: "Brainfuck",
    extensions: ["b", "bf"],
    load() {
      return import("./brainfuck-X62PLIM7.js").then((m) => legacy(m.brainfuck));
    }
  }),
  LanguageDescription.of({
    name: "Cobol",
    extensions: ["cob", "cpy"],
    load() {
      return import("./cobol-INVJ5RXR.js").then((m) => legacy(m.cobol));
    }
  }),
  LanguageDescription.of({
    name: "C#",
    alias: ["csharp", "cs"],
    extensions: ["cs"],
    load() {
      return import("./clike-U32CMODL.js").then((m) => legacy(m.csharp));
    }
  }),
  LanguageDescription.of({
    name: "Clojure",
    extensions: ["clj", "cljc", "cljx"],
    load() {
      return import("./clojure-2C5MWSC6.js").then((m) => legacy(m.clojure));
    }
  }),
  LanguageDescription.of({
    name: "ClojureScript",
    extensions: ["cljs"],
    load() {
      return import("./clojure-2C5MWSC6.js").then((m) => legacy(m.clojure));
    }
  }),
  LanguageDescription.of({
    name: "Closure Stylesheets (GSS)",
    extensions: ["gss"],
    load() {
      return import("./css-HUW2XNTY.js").then((m) => legacy(m.gss));
    }
  }),
  LanguageDescription.of({
    name: "CMake",
    extensions: ["cmake", "cmake.in"],
    filename: /^CMakeLists\.txt$/,
    load() {
      return import("./cmake-JYSQCCBU.js").then((m) => legacy(m.cmake));
    }
  }),
  LanguageDescription.of({
    name: "CoffeeScript",
    alias: ["coffee", "coffee-script"],
    extensions: ["coffee"],
    load() {
      return import("./coffeescript-A4LLXCQF.js").then((m) => legacy(m.coffeeScript));
    }
  }),
  LanguageDescription.of({
    name: "Common Lisp",
    alias: ["lisp"],
    extensions: ["cl", "lisp", "el"],
    load() {
      return import("./commonlisp-5XX4SCPS.js").then((m) => legacy(m.commonLisp));
    }
  }),
  LanguageDescription.of({
    name: "Cypher",
    extensions: ["cyp", "cypher"],
    load() {
      return import("./cypher-Y33CMNK6.js").then((m) => legacy(m.cypher));
    }
  }),
  LanguageDescription.of({
    name: "Cython",
    extensions: ["pyx", "pxd", "pxi"],
    load() {
      return import("./python-SJF75366.js").then((m) => legacy(m.cython));
    }
  }),
  LanguageDescription.of({
    name: "Crystal",
    extensions: ["cr"],
    load() {
      return import("./crystal-W3JY7H2F.js").then((m) => legacy(m.crystal));
    }
  }),
  LanguageDescription.of({
    name: "D",
    extensions: ["d"],
    load() {
      return import("./d-HOVZ6YKT.js").then((m) => legacy(m.d));
    }
  }),
  LanguageDescription.of({
    name: "Dart",
    extensions: ["dart"],
    load() {
      return import("./clike-U32CMODL.js").then((m) => legacy(m.dart));
    }
  }),
  LanguageDescription.of({
    name: "diff",
    extensions: ["diff", "patch"],
    load() {
      return import("./diff-45S37XFH.js").then((m) => legacy(m.diff));
    }
  }),
  LanguageDescription.of({
    name: "Dockerfile",
    filename: /^Dockerfile$/,
    load() {
      return import("./dockerfile-Z7YHSJIX.js").then((m) => legacy(m.dockerFile));
    }
  }),
  LanguageDescription.of({
    name: "DTD",
    extensions: ["dtd"],
    load() {
      return import("./dtd-FYAJBXFW.js").then((m) => legacy(m.dtd));
    }
  }),
  LanguageDescription.of({
    name: "Dylan",
    extensions: ["dylan", "dyl", "intr"],
    load() {
      return import("./dylan-6IXKS42J.js").then((m) => legacy(m.dylan));
    }
  }),
  LanguageDescription.of({
    name: "EBNF",
    load() {
      return import("./ebnf-J3DYJXQK.js").then((m) => legacy(m.ebnf));
    }
  }),
  LanguageDescription.of({
    name: "ECL",
    extensions: ["ecl"],
    load() {
      return import("./ecl-W7B2VSKQ.js").then((m) => legacy(m.ecl));
    }
  }),
  LanguageDescription.of({
    name: "edn",
    extensions: ["edn"],
    load() {
      return import("./clojure-2C5MWSC6.js").then((m) => legacy(m.clojure));
    }
  }),
  LanguageDescription.of({
    name: "Eiffel",
    extensions: ["e"],
    load() {
      return import("./eiffel-X3LURPGC.js").then((m) => legacy(m.eiffel));
    }
  }),
  LanguageDescription.of({
    name: "Elm",
    extensions: ["elm"],
    load() {
      return import("./elm-IJPIPGF7.js").then((m) => legacy(m.elm));
    }
  }),
  LanguageDescription.of({
    name: "Erlang",
    extensions: ["erl"],
    load() {
      return import("./erlang-EVPQWQZU.js").then((m) => legacy(m.erlang));
    }
  }),
  LanguageDescription.of({
    name: "Esper",
    load() {
      return import("./sql-2K3GBG6P.js").then((m) => legacy(m.esper));
    }
  }),
  LanguageDescription.of({
    name: "Factor",
    extensions: ["factor"],
    load() {
      return import("./factor-U25U5RYS.js").then((m) => legacy(m.factor));
    }
  }),
  LanguageDescription.of({
    name: "FCL",
    load() {
      return import("./fcl-PBIJISFJ.js").then((m) => legacy(m.fcl));
    }
  }),
  LanguageDescription.of({
    name: "Forth",
    extensions: ["forth", "fth", "4th"],
    load() {
      return import("./forth-LHWTWK4R.js").then((m) => legacy(m.forth));
    }
  }),
  LanguageDescription.of({
    name: "Fortran",
    extensions: ["f", "for", "f77", "f90", "f95"],
    load() {
      return import("./fortran-YW23FMA5.js").then((m) => legacy(m.fortran));
    }
  }),
  LanguageDescription.of({
    name: "F#",
    alias: ["fsharp"],
    extensions: ["fs"],
    load() {
      return import("./mllike-43NGLPHQ.js").then((m) => legacy(m.fSharp));
    }
  }),
  LanguageDescription.of({
    name: "Gas",
    extensions: ["s"],
    load() {
      return import("./gas-HMOAUBEO.js").then((m) => legacy(m.gas));
    }
  }),
  LanguageDescription.of({
    name: "Gherkin",
    extensions: ["feature"],
    load() {
      return import("./gherkin-JEEGIJFU.js").then((m) => legacy(m.gherkin));
    }
  }),
  LanguageDescription.of({
    name: "Groovy",
    extensions: ["groovy", "gradle"],
    filename: /^Jenkinsfile$/,
    load() {
      return import("./groovy-VYOK23RL.js").then((m) => legacy(m.groovy));
    }
  }),
  LanguageDescription.of({
    name: "Haskell",
    extensions: ["hs"],
    load() {
      return import("./haskell-OOMNH52N.js").then((m) => legacy(m.haskell));
    }
  }),
  LanguageDescription.of({
    name: "Haxe",
    extensions: ["hx"],
    load() {
      return import("./haxe-YTO22ALR.js").then((m) => legacy(m.haxe));
    }
  }),
  LanguageDescription.of({
    name: "HXML",
    extensions: ["hxml"],
    load() {
      return import("./haxe-YTO22ALR.js").then((m) => legacy(m.hxml));
    }
  }),
  LanguageDescription.of({
    name: "HTTP",
    load() {
      return import("./http-HR4OJSOJ.js").then((m) => legacy(m.http));
    }
  }),
  LanguageDescription.of({
    name: "IDL",
    extensions: ["pro"],
    load() {
      return import("./idl-DQUHBULU.js").then((m) => legacy(m.idl));
    }
  }),
  LanguageDescription.of({
    name: "JSON-LD",
    alias: ["jsonld"],
    extensions: ["jsonld"],
    load() {
      return import("./javascript-QLIYEFBA.js").then((m) => legacy(m.jsonld));
    }
  }),
  LanguageDescription.of({
    name: "Julia",
    extensions: ["jl"],
    load() {
      return import("./julia-OZ5Y4UCW.js").then((m) => legacy(m.julia));
    }
  }),
  LanguageDescription.of({
    name: "Kotlin",
    extensions: ["kt", "kts"],
    load() {
      return import("./clike-U32CMODL.js").then((m) => legacy(m.kotlin));
    }
  }),
  LanguageDescription.of({
    name: "LiveScript",
    alias: ["ls"],
    extensions: ["ls"],
    load() {
      return import("./livescript-2R5MCK6D.js").then((m) => legacy(m.liveScript));
    }
  }),
  LanguageDescription.of({
    name: "Lua",
    extensions: ["lua"],
    load() {
      return import("./lua-7XV4GEAE.js").then((m) => legacy(m.lua));
    }
  }),
  LanguageDescription.of({
    name: "mIRC",
    extensions: ["mrc"],
    load() {
      return import("./mirc-YDPNMSCK.js").then((m) => legacy(m.mirc));
    }
  }),
  LanguageDescription.of({
    name: "Mathematica",
    extensions: ["m", "nb", "wl", "wls"],
    load() {
      return import("./mathematica-VSEDG74T.js").then((m) => legacy(m.mathematica));
    }
  }),
  LanguageDescription.of({
    name: "Modelica",
    extensions: ["mo"],
    load() {
      return import("./modelica-7Y2BMTX4.js").then((m) => legacy(m.modelica));
    }
  }),
  LanguageDescription.of({
    name: "MUMPS",
    extensions: ["mps"],
    load() {
      return import("./mumps-X3TPUVXN.js").then((m) => legacy(m.mumps));
    }
  }),
  LanguageDescription.of({
    name: "Mbox",
    extensions: ["mbox"],
    load() {
      return import("./mbox-VUYW6IGG.js").then((m) => legacy(m.mbox));
    }
  }),
  LanguageDescription.of({
    name: "Nginx",
    filename: /nginx.*\.conf$/i,
    load() {
      return import("./nginx-JOEUPARX.js").then((m) => legacy(m.nginx));
    }
  }),
  LanguageDescription.of({
    name: "NSIS",
    extensions: ["nsh", "nsi"],
    load() {
      return import("./nsis-6AW3OWAX.js").then((m) => legacy(m.nsis));
    }
  }),
  LanguageDescription.of({
    name: "NTriples",
    extensions: ["nt", "nq"],
    load() {
      return import("./ntriples-3ROXNWS5.js").then((m) => legacy(m.ntriples));
    }
  }),
  LanguageDescription.of({
    name: "Objective-C",
    alias: ["objective-c", "objc"],
    extensions: ["m"],
    load() {
      return import("./clike-U32CMODL.js").then((m) => legacy(m.objectiveC));
    }
  }),
  LanguageDescription.of({
    name: "Objective-C++",
    alias: ["objective-c++", "objc++"],
    extensions: ["mm"],
    load() {
      return import("./clike-U32CMODL.js").then((m) => legacy(m.objectiveCpp));
    }
  }),
  LanguageDescription.of({
    name: "OCaml",
    extensions: ["ml", "mli", "mll", "mly"],
    load() {
      return import("./mllike-43NGLPHQ.js").then((m) => legacy(m.oCaml));
    }
  }),
  LanguageDescription.of({
    name: "Octave",
    extensions: ["m"],
    load() {
      return import("./octave-LESHCCFL.js").then((m) => legacy(m.octave));
    }
  }),
  LanguageDescription.of({
    name: "Oz",
    extensions: ["oz"],
    load() {
      return import("./oz-H5EDZ6NJ.js").then((m) => legacy(m.oz));
    }
  }),
  LanguageDescription.of({
    name: "Pascal",
    extensions: ["p", "pas"],
    load() {
      return import("./pascal-V7WTHQGC.js").then((m) => legacy(m.pascal));
    }
  }),
  LanguageDescription.of({
    name: "Perl",
    extensions: ["pl", "pm"],
    load() {
      return import("./perl-3SIF577F.js").then((m) => legacy(m.perl));
    }
  }),
  LanguageDescription.of({
    name: "Pig",
    extensions: ["pig"],
    load() {
      return import("./pig-5IOQWVQU.js").then((m) => legacy(m.pig));
    }
  }),
  LanguageDescription.of({
    name: "PowerShell",
    extensions: ["ps1", "psd1", "psm1"],
    load() {
      return import("./powershell-GVBVH7GM.js").then((m) => legacy(m.powerShell));
    }
  }),
  LanguageDescription.of({
    name: "Properties files",
    alias: ["ini", "properties"],
    extensions: ["properties", "ini", "in"],
    load() {
      return import("./properties-WN775ER4.js").then((m) => legacy(m.properties));
    }
  }),
  LanguageDescription.of({
    name: "ProtoBuf",
    extensions: ["proto"],
    load() {
      return import("./protobuf-BRDE5HKD.js").then((m) => legacy(m.protobuf));
    }
  }),
  LanguageDescription.of({
    name: "Pug",
    alias: ["jade"],
    extensions: ["pug", "jade"],
    load() {
      return import("./pug-6XCJXDWV.js").then((m) => legacy(m.pug));
    }
  }),
  LanguageDescription.of({
    name: "Puppet",
    extensions: ["pp"],
    load() {
      return import("./puppet-HI72Y2JP.js").then((m) => legacy(m.puppet));
    }
  }),
  LanguageDescription.of({
    name: "Q",
    extensions: ["q"],
    load() {
      return import("./q-MGHWTPOH.js").then((m) => legacy(m.q));
    }
  }),
  LanguageDescription.of({
    name: "R",
    alias: ["rscript"],
    extensions: ["r", "R"],
    load() {
      return import("./r-4SDJANML.js").then((m) => legacy(m.r));
    }
  }),
  LanguageDescription.of({
    name: "RPM Changes",
    load() {
      return import("./rpm-MJ2SMQMT.js").then((m) => legacy(m.rpmChanges));
    }
  }),
  LanguageDescription.of({
    name: "RPM Spec",
    extensions: ["spec"],
    load() {
      return import("./rpm-MJ2SMQMT.js").then((m) => legacy(m.rpmSpec));
    }
  }),
  LanguageDescription.of({
    name: "Ruby",
    alias: ["jruby", "macruby", "rake", "rb", "rbx"],
    extensions: ["rb"],
    filename: /^(Gemfile|Rakefile)$/,
    load() {
      return import("./ruby-7AMJBLPP.js").then((m) => legacy(m.ruby));
    }
  }),
  LanguageDescription.of({
    name: "SAS",
    extensions: ["sas"],
    load() {
      return import("./sas-7W2ZX2EF.js").then((m) => legacy(m.sas));
    }
  }),
  LanguageDescription.of({
    name: "Scala",
    extensions: ["scala"],
    load() {
      return import("./clike-U32CMODL.js").then((m) => legacy(m.scala));
    }
  }),
  LanguageDescription.of({
    name: "Scheme",
    extensions: ["scm", "ss"],
    load() {
      return import("./scheme-DEGWFWGR.js").then((m) => legacy(m.scheme));
    }
  }),
  LanguageDescription.of({
    name: "Shell",
    alias: ["bash", "sh", "zsh"],
    extensions: ["sh", "ksh", "bash"],
    filename: /^PKGBUILD$/,
    load() {
      return import("./shell-VP64EXIH.js").then((m) => legacy(m.shell));
    }
  }),
  LanguageDescription.of({
    name: "Sieve",
    extensions: ["siv", "sieve"],
    load() {
      return import("./sieve-56XSCIJY.js").then((m) => legacy(m.sieve));
    }
  }),
  LanguageDescription.of({
    name: "Smalltalk",
    extensions: ["st"],
    load() {
      return import("./smalltalk-SO3EO23W.js").then((m) => legacy(m.smalltalk));
    }
  }),
  LanguageDescription.of({
    name: "Solr",
    load() {
      return import("./solr-XMHZTAP3.js").then((m) => legacy(m.solr));
    }
  }),
  LanguageDescription.of({
    name: "SML",
    extensions: ["sml", "sig", "fun", "smackspec"],
    load() {
      return import("./mllike-43NGLPHQ.js").then((m) => legacy(m.sml));
    }
  }),
  LanguageDescription.of({
    name: "SPARQL",
    alias: ["sparul"],
    extensions: ["rq", "sparql"],
    load() {
      return import("./sparql-27V4ILVO.js").then((m) => legacy(m.sparql));
    }
  }),
  LanguageDescription.of({
    name: "Spreadsheet",
    alias: ["excel", "formula"],
    load() {
      return import("./spreadsheet-JBOC5VCR.js").then((m) => legacy(m.spreadsheet));
    }
  }),
  LanguageDescription.of({
    name: "Squirrel",
    extensions: ["nut"],
    load() {
      return import("./clike-U32CMODL.js").then((m) => legacy(m.squirrel));
    }
  }),
  LanguageDescription.of({
    name: "Stylus",
    extensions: ["styl"],
    load() {
      return import("./stylus-IMH4O5OF.js").then((m) => legacy(m.stylus));
    }
  }),
  LanguageDescription.of({
    name: "Swift",
    extensions: ["swift"],
    load() {
      return import("./swift-GOSEPKJH.js").then((m) => legacy(m.swift));
    }
  }),
  LanguageDescription.of({
    name: "sTeX",
    load() {
      return import("./stex-ZSJJWWG2.js").then((m) => legacy(m.stex));
    }
  }),
  LanguageDescription.of({
    name: "LaTeX",
    alias: ["tex"],
    extensions: ["text", "ltx", "tex"],
    load() {
      return import("./stex-ZSJJWWG2.js").then((m) => legacy(m.stex));
    }
  }),
  LanguageDescription.of({
    name: "SystemVerilog",
    extensions: ["v", "sv", "svh"],
    load() {
      return import("./verilog-3QSMSNAQ.js").then((m) => legacy(m.verilog));
    }
  }),
  LanguageDescription.of({
    name: "Tcl",
    extensions: ["tcl"],
    load() {
      return import("./tcl-P7KZVHAX.js").then((m) => legacy(m.tcl));
    }
  }),
  LanguageDescription.of({
    name: "Textile",
    extensions: ["textile"],
    load() {
      return import("./textile-W4CN77NS.js").then((m) => legacy(m.textile));
    }
  }),
  LanguageDescription.of({
    name: "TiddlyWiki",
    load() {
      return import("./tiddlywiki-MMOZDHIJ.js").then((m) => legacy(m.tiddlyWiki));
    }
  }),
  LanguageDescription.of({
    name: "Tiki wiki",
    load() {
      return import("./tiki-PSYSISUZ.js").then((m) => legacy(m.tiki));
    }
  }),
  LanguageDescription.of({
    name: "TOML",
    extensions: ["toml"],
    load() {
      return import("./toml-54AACD6Y.js").then((m) => legacy(m.toml));
    }
  }),
  LanguageDescription.of({
    name: "Troff",
    extensions: ["1", "2", "3", "4", "5", "6", "7", "8", "9"],
    load() {
      return import("./troff-5XH5XGO4.js").then((m) => legacy(m.troff));
    }
  }),
  LanguageDescription.of({
    name: "TTCN",
    extensions: ["ttcn", "ttcn3", "ttcnpp"],
    load() {
      return import("./ttcn-TB5IDNPW.js").then((m) => legacy(m.ttcn));
    }
  }),
  LanguageDescription.of({
    name: "TTCN_CFG",
    extensions: ["cfg"],
    load() {
      return import("./ttcn-cfg-N3G5TA37.js").then((m) => legacy(m.ttcnCfg));
    }
  }),
  LanguageDescription.of({
    name: "Turtle",
    extensions: ["ttl"],
    load() {
      return import("./turtle-7O3K5V4Q.js").then((m) => legacy(m.turtle));
    }
  }),
  LanguageDescription.of({
    name: "Web IDL",
    extensions: ["webidl"],
    load() {
      return import("./webidl-7DQCD4XN.js").then((m) => legacy(m.webIDL));
    }
  }),
  LanguageDescription.of({
    name: "VB.NET",
    extensions: ["vb"],
    load() {
      return import("./vb-6U4DEFFW.js").then((m) => legacy(m.vb));
    }
  }),
  LanguageDescription.of({
    name: "VBScript",
    extensions: ["vbs"],
    load() {
      return import("./vbscript-ZMFYPZTH.js").then((m) => legacy(m.vbScript));
    }
  }),
  LanguageDescription.of({
    name: "Velocity",
    extensions: ["vtl"],
    load() {
      return import("./velocity-AHUIPALW.js").then((m) => legacy(m.velocity));
    }
  }),
  LanguageDescription.of({
    name: "Verilog",
    extensions: ["v"],
    load() {
      return import("./verilog-3QSMSNAQ.js").then((m) => legacy(m.verilog));
    }
  }),
  LanguageDescription.of({
    name: "VHDL",
    extensions: ["vhd", "vhdl"],
    load() {
      return import("./vhdl-7J7ZQK2K.js").then((m) => legacy(m.vhdl));
    }
  }),
  LanguageDescription.of({
    name: "XQuery",
    extensions: ["xy", "xquery", "xq", "xqm", "xqy"],
    load() {
      return import("./xquery-H2FM6QL4.js").then((m) => legacy(m.xQuery));
    }
  }),
  LanguageDescription.of({
    name: "Yacas",
    extensions: ["ys"],
    load() {
      return import("./yacas-H6MFCEA5.js").then((m) => legacy(m.yacas));
    }
  }),
  LanguageDescription.of({
    name: "Z80",
    extensions: ["z80"],
    load() {
      return import("./z80-XZHMYOYW.js").then((m) => legacy(m.z80));
    }
  }),
  LanguageDescription.of({
    name: "MscGen",
    extensions: ["mscgen", "mscin", "msc"],
    load() {
      return import("./mscgen-KNKFMJR3.js").then((m) => legacy(m.mscgen));
    }
  }),
  LanguageDescription.of({
    name: "Xù",
    extensions: ["xu"],
    load() {
      return import("./mscgen-KNKFMJR3.js").then((m) => legacy(m.xu));
    }
  }),
  LanguageDescription.of({
    name: "MsGenny",
    extensions: ["msgenny"],
    load() {
      return import("./mscgen-KNKFMJR3.js").then((m) => legacy(m.msgenny));
    }
  }),
  LanguageDescription.of({
    name: "Vue",
    extensions: ["vue"],
    load() {
      return import("./dist-LRRZ6HZV.js").then((m) => m.vue());
    }
  }),
  LanguageDescription.of({
    name: "Angular Template",
    load() {
      return import("./dist-3DJ3Z7LN.js").then((m) => m.angular());
    }
  })
];
export {
  languages
};
//# sourceMappingURL=@codemirror_language-data.js.map

import {
  LanguageDescription,
  LanguageSupport,
  StreamLanguage
} from "./chunk-OAQSJFBI.js";
import "./chunk-IZ7ZILRD.js";
import "./chunk-DVO2Z5C3.js";
import "./chunk-M5REAYG5.js";
import "./chunk-GJW6Z6K3.js";
import "./chunk-PR4QN5HX.js";

// node_modules/@codemirror/language-data/dist/index.js
function legacy(parser) {
  return new LanguageSupport(StreamLanguage.define(parser));
}
function sql(dialectName) {
  return import("./dist-M2ZTOPEP.js").then((m) => m.sql({ dialect: m[dialectName] }));
}
var languages = [
  // New-style language modes
  LanguageDescription.of({
    name: "C",
    extensions: ["c", "h", "ino"],
    load() {
      return import("./dist-6GRCRQPA.js").then((m) => m.cpp());
    }
  }),
  LanguageDescription.of({
    name: "C++",
    alias: ["cpp"],
    extensions: ["cpp", "c++", "cc", "cxx", "hpp", "h++", "hh", "hxx"],
    load() {
      return import("./dist-6GRCRQPA.js").then((m) => m.cpp());
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
      return import("./dist-CO3HWEFC.js").then((m) => m.css());
    }
  }),
  LanguageDescription.of({
    name: "Go",
    extensions: ["go"],
    load() {
      return import("./dist-J27MHVVT.js").then((m) => m.go());
    }
  }),
  LanguageDescription.of({
    name: "HTML",
    alias: ["xhtml"],
    extensions: ["html", "htm", "handlebars", "hbs"],
    load() {
      return import("./dist-CAKUPVHZ.js").then((m) => m.html());
    }
  }),
  LanguageDescription.of({
    name: "Java",
    extensions: ["java"],
    load() {
      return import("./dist-UBYIYQHT.js").then((m) => m.java());
    }
  }),
  LanguageDescription.of({
    name: "JavaScript",
    alias: ["ecmascript", "js", "node"],
    extensions: ["js", "mjs", "cjs"],
    load() {
      return import("./dist-6GGDB5TP.js").then((m) => m.javascript());
    }
  }),
  LanguageDescription.of({
    name: "Jinja",
    extensions: ["j2", "jinja", "jinja2"],
    load() {
      return import("./dist-H3TY5G7J.js").then((m) => m.jinja());
    }
  }),
  LanguageDescription.of({
    name: "JSON",
    alias: ["json5"],
    extensions: ["json", "map"],
    load() {
      return import("./dist-EW6P36YV.js").then((m) => m.json());
    }
  }),
  LanguageDescription.of({
    name: "JSX",
    extensions: ["jsx"],
    load() {
      return import("./dist-6GGDB5TP.js").then((m) => m.javascript({ jsx: true }));
    }
  }),
  LanguageDescription.of({
    name: "LESS",
    extensions: ["less"],
    load() {
      return import("./dist-GTE22GR5.js").then((m) => m.less());
    }
  }),
  LanguageDescription.of({
    name: "Liquid",
    extensions: ["liquid"],
    load() {
      return import("./dist-LHXOJCYO.js").then((m) => m.liquid());
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
      return import("./dist-MDIIQ5B5.js").then((m) => m.php());
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
      return import("./dist-PWLYHFCS.js").then((m) => m.python());
    }
  }),
  LanguageDescription.of({
    name: "Rust",
    extensions: ["rs"],
    load() {
      return import("./dist-AGJOWO7W.js").then((m) => m.rust());
    }
  }),
  LanguageDescription.of({
    name: "Sass",
    extensions: ["sass"],
    load() {
      return import("./dist-3JNHXP3N.js").then((m) => m.sass({ indented: true }));
    }
  }),
  LanguageDescription.of({
    name: "SCSS",
    extensions: ["scss"],
    load() {
      return import("./dist-3JNHXP3N.js").then((m) => m.sass());
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
      return import("./dist-6GGDB5TP.js").then((m) => m.javascript({ jsx: true, typescript: true }));
    }
  }),
  LanguageDescription.of({
    name: "TypeScript",
    alias: ["ts"],
    extensions: ["ts", "mts", "cts"],
    load() {
      return import("./dist-6GGDB5TP.js").then((m) => m.javascript({ typescript: true }));
    }
  }),
  LanguageDescription.of({
    name: "WebAssembly",
    extensions: ["wat", "wast"],
    load() {
      return import("./dist-TCABPJER.js").then((m) => m.wast());
    }
  }),
  LanguageDescription.of({
    name: "XML",
    alias: ["rss", "wsdl", "xsd"],
    extensions: ["xml", "xsl", "xsd", "svg"],
    load() {
      return import("./dist-GDOPPTFP.js").then((m) => m.xml());
    }
  }),
  LanguageDescription.of({
    name: "YAML",
    alias: ["yml"],
    extensions: ["yaml", "yml"],
    load() {
      return import("./dist-HWTVLSX2.js").then((m) => m.yaml());
    }
  }),
  // Legacy modes ported from CodeMirror 5
  LanguageDescription.of({
    name: "APL",
    extensions: ["dyalog", "apl"],
    load() {
      return import("./apl-3H6FBUZD.js").then((m) => legacy(m.apl));
    }
  }),
  LanguageDescription.of({
    name: "PGP",
    alias: ["asciiarmor"],
    extensions: ["asc", "pgp", "sig"],
    load() {
      return import("./asciiarmor-RPTCEQZ3.js").then((m) => legacy(m.asciiArmor));
    }
  }),
  LanguageDescription.of({
    name: "ASN.1",
    extensions: ["asn", "asn1"],
    load() {
      return import("./asn1-GIK5P7Z2.js").then((m) => legacy(m.asn1({})));
    }
  }),
  LanguageDescription.of({
    name: "Asterisk",
    filename: /^extensions\.conf$/i,
    load() {
      return import("./asterisk-3MBSACJK.js").then((m) => legacy(m.asterisk));
    }
  }),
  LanguageDescription.of({
    name: "Brainfuck",
    extensions: ["b", "bf"],
    load() {
      return import("./brainfuck-WPJPBX3F.js").then((m) => legacy(m.brainfuck));
    }
  }),
  LanguageDescription.of({
    name: "Cobol",
    extensions: ["cob", "cpy"],
    load() {
      return import("./cobol-323RFDXB.js").then((m) => legacy(m.cobol));
    }
  }),
  LanguageDescription.of({
    name: "C#",
    alias: ["csharp", "cs"],
    extensions: ["cs"],
    load() {
      return import("./clike-ZGN5GAC6.js").then((m) => legacy(m.csharp));
    }
  }),
  LanguageDescription.of({
    name: "Clojure",
    extensions: ["clj", "cljc", "cljx"],
    load() {
      return import("./clojure-EOYI3XZ5.js").then((m) => legacy(m.clojure));
    }
  }),
  LanguageDescription.of({
    name: "ClojureScript",
    extensions: ["cljs"],
    load() {
      return import("./clojure-EOYI3XZ5.js").then((m) => legacy(m.clojure));
    }
  }),
  LanguageDescription.of({
    name: "Closure Stylesheets (GSS)",
    extensions: ["gss"],
    load() {
      return import("./css-E6DKMKEN.js").then((m) => legacy(m.gss));
    }
  }),
  LanguageDescription.of({
    name: "CMake",
    extensions: ["cmake", "cmake.in"],
    filename: /^CMakeLists\.txt$/,
    load() {
      return import("./cmake-5RUBQVGY.js").then((m) => legacy(m.cmake));
    }
  }),
  LanguageDescription.of({
    name: "CoffeeScript",
    alias: ["coffee", "coffee-script"],
    extensions: ["coffee"],
    load() {
      return import("./coffeescript-4ZXRXR7I.js").then((m) => legacy(m.coffeeScript));
    }
  }),
  LanguageDescription.of({
    name: "Common Lisp",
    alias: ["lisp"],
    extensions: ["cl", "lisp", "el"],
    load() {
      return import("./commonlisp-PB73JXRB.js").then((m) => legacy(m.commonLisp));
    }
  }),
  LanguageDescription.of({
    name: "Cypher",
    extensions: ["cyp", "cypher"],
    load() {
      return import("./cypher-ZJBHLX3G.js").then((m) => legacy(m.cypher));
    }
  }),
  LanguageDescription.of({
    name: "Cython",
    extensions: ["pyx", "pxd", "pxi"],
    load() {
      return import("./python-TKGPZGXD.js").then((m) => legacy(m.cython));
    }
  }),
  LanguageDescription.of({
    name: "Crystal",
    extensions: ["cr"],
    load() {
      return import("./crystal-STFYIYBT.js").then((m) => legacy(m.crystal));
    }
  }),
  LanguageDescription.of({
    name: "D",
    extensions: ["d"],
    load() {
      return import("./d-YP4KEIG6.js").then((m) => legacy(m.d));
    }
  }),
  LanguageDescription.of({
    name: "Dart",
    extensions: ["dart"],
    load() {
      return import("./clike-ZGN5GAC6.js").then((m) => legacy(m.dart));
    }
  }),
  LanguageDescription.of({
    name: "diff",
    extensions: ["diff", "patch"],
    load() {
      return import("./diff-VHRUQOTN.js").then((m) => legacy(m.diff));
    }
  }),
  LanguageDescription.of({
    name: "Dockerfile",
    filename: /^Dockerfile$/,
    load() {
      return import("./dockerfile-YGPAWEBB.js").then((m) => legacy(m.dockerFile));
    }
  }),
  LanguageDescription.of({
    name: "DTD",
    extensions: ["dtd"],
    load() {
      return import("./dtd-QQUYUG5L.js").then((m) => legacy(m.dtd));
    }
  }),
  LanguageDescription.of({
    name: "Dylan",
    extensions: ["dylan", "dyl", "intr"],
    load() {
      return import("./dylan-YUNDXYTD.js").then((m) => legacy(m.dylan));
    }
  }),
  LanguageDescription.of({
    name: "EBNF",
    load() {
      return import("./ebnf-TDQKCPB6.js").then((m) => legacy(m.ebnf));
    }
  }),
  LanguageDescription.of({
    name: "ECL",
    extensions: ["ecl"],
    load() {
      return import("./ecl-2T4BJVOH.js").then((m) => legacy(m.ecl));
    }
  }),
  LanguageDescription.of({
    name: "edn",
    extensions: ["edn"],
    load() {
      return import("./clojure-EOYI3XZ5.js").then((m) => legacy(m.clojure));
    }
  }),
  LanguageDescription.of({
    name: "Eiffel",
    extensions: ["e"],
    load() {
      return import("./eiffel-E7TERKCG.js").then((m) => legacy(m.eiffel));
    }
  }),
  LanguageDescription.of({
    name: "Elm",
    extensions: ["elm"],
    load() {
      return import("./elm-DXLQIK57.js").then((m) => legacy(m.elm));
    }
  }),
  LanguageDescription.of({
    name: "Erlang",
    extensions: ["erl"],
    load() {
      return import("./erlang-RTQZZ4W4.js").then((m) => legacy(m.erlang));
    }
  }),
  LanguageDescription.of({
    name: "Esper",
    load() {
      return import("./sql-A2T2XSJ2.js").then((m) => legacy(m.esper));
    }
  }),
  LanguageDescription.of({
    name: "Factor",
    extensions: ["factor"],
    load() {
      return import("./factor-4BA2LUFG.js").then((m) => legacy(m.factor));
    }
  }),
  LanguageDescription.of({
    name: "FCL",
    load() {
      return import("./fcl-TKDFP7WU.js").then((m) => legacy(m.fcl));
    }
  }),
  LanguageDescription.of({
    name: "Forth",
    extensions: ["forth", "fth", "4th"],
    load() {
      return import("./forth-5JPUAXJG.js").then((m) => legacy(m.forth));
    }
  }),
  LanguageDescription.of({
    name: "Fortran",
    extensions: ["f", "for", "f77", "f90", "f95"],
    load() {
      return import("./fortran-75BDFRM7.js").then((m) => legacy(m.fortran));
    }
  }),
  LanguageDescription.of({
    name: "F#",
    alias: ["fsharp"],
    extensions: ["fs"],
    load() {
      return import("./mllike-QM2KT336.js").then((m) => legacy(m.fSharp));
    }
  }),
  LanguageDescription.of({
    name: "Gas",
    extensions: ["s"],
    load() {
      return import("./gas-23674C6C.js").then((m) => legacy(m.gas));
    }
  }),
  LanguageDescription.of({
    name: "Gherkin",
    extensions: ["feature"],
    load() {
      return import("./gherkin-KVSBFWKS.js").then((m) => legacy(m.gherkin));
    }
  }),
  LanguageDescription.of({
    name: "Groovy",
    extensions: ["groovy", "gradle"],
    filename: /^Jenkinsfile$/,
    load() {
      return import("./groovy-CA6PX4N6.js").then((m) => legacy(m.groovy));
    }
  }),
  LanguageDescription.of({
    name: "Haskell",
    extensions: ["hs"],
    load() {
      return import("./haskell-DZTBRF7Z.js").then((m) => legacy(m.haskell));
    }
  }),
  LanguageDescription.of({
    name: "Haxe",
    extensions: ["hx"],
    load() {
      return import("./haxe-S3MGTBJL.js").then((m) => legacy(m.haxe));
    }
  }),
  LanguageDescription.of({
    name: "HXML",
    extensions: ["hxml"],
    load() {
      return import("./haxe-S3MGTBJL.js").then((m) => legacy(m.hxml));
    }
  }),
  LanguageDescription.of({
    name: "HTTP",
    load() {
      return import("./http-VZ3I7OUX.js").then((m) => legacy(m.http));
    }
  }),
  LanguageDescription.of({
    name: "IDL",
    extensions: ["pro"],
    load() {
      return import("./idl-3E2FJ5Q2.js").then((m) => legacy(m.idl));
    }
  }),
  LanguageDescription.of({
    name: "JSON-LD",
    alias: ["jsonld"],
    extensions: ["jsonld"],
    load() {
      return import("./javascript-6R5V7IXH.js").then((m) => legacy(m.jsonld));
    }
  }),
  LanguageDescription.of({
    name: "Julia",
    extensions: ["jl"],
    load() {
      return import("./julia-FMSMYYM5.js").then((m) => legacy(m.julia));
    }
  }),
  LanguageDescription.of({
    name: "Kotlin",
    extensions: ["kt", "kts"],
    load() {
      return import("./clike-ZGN5GAC6.js").then((m) => legacy(m.kotlin));
    }
  }),
  LanguageDescription.of({
    name: "LiveScript",
    alias: ["ls"],
    extensions: ["ls"],
    load() {
      return import("./livescript-LLJA23V3.js").then((m) => legacy(m.liveScript));
    }
  }),
  LanguageDescription.of({
    name: "Lua",
    extensions: ["lua"],
    load() {
      return import("./lua-CSBKXQX2.js").then((m) => legacy(m.lua));
    }
  }),
  LanguageDescription.of({
    name: "mIRC",
    extensions: ["mrc"],
    load() {
      return import("./mirc-LKAZL2NR.js").then((m) => legacy(m.mirc));
    }
  }),
  LanguageDescription.of({
    name: "Mathematica",
    extensions: ["m", "nb", "wl", "wls"],
    load() {
      return import("./mathematica-O3TUJSUN.js").then((m) => legacy(m.mathematica));
    }
  }),
  LanguageDescription.of({
    name: "Modelica",
    extensions: ["mo"],
    load() {
      return import("./modelica-BEVMQ6Y3.js").then((m) => legacy(m.modelica));
    }
  }),
  LanguageDescription.of({
    name: "MUMPS",
    extensions: ["mps"],
    load() {
      return import("./mumps-7PC7PBWG.js").then((m) => legacy(m.mumps));
    }
  }),
  LanguageDescription.of({
    name: "Mbox",
    extensions: ["mbox"],
    load() {
      return import("./mbox-UZKRJCNM.js").then((m) => legacy(m.mbox));
    }
  }),
  LanguageDescription.of({
    name: "Nginx",
    filename: /nginx.*\.conf$/i,
    load() {
      return import("./nginx-EAEED2SH.js").then((m) => legacy(m.nginx));
    }
  }),
  LanguageDescription.of({
    name: "NSIS",
    extensions: ["nsh", "nsi"],
    load() {
      return import("./nsis-KHQEK6TS.js").then((m) => legacy(m.nsis));
    }
  }),
  LanguageDescription.of({
    name: "NTriples",
    extensions: ["nt", "nq"],
    load() {
      return import("./ntriples-OFVZHKLW.js").then((m) => legacy(m.ntriples));
    }
  }),
  LanguageDescription.of({
    name: "Objective-C",
    alias: ["objective-c", "objc"],
    extensions: ["m"],
    load() {
      return import("./clike-ZGN5GAC6.js").then((m) => legacy(m.objectiveC));
    }
  }),
  LanguageDescription.of({
    name: "Objective-C++",
    alias: ["objective-c++", "objc++"],
    extensions: ["mm"],
    load() {
      return import("./clike-ZGN5GAC6.js").then((m) => legacy(m.objectiveCpp));
    }
  }),
  LanguageDescription.of({
    name: "OCaml",
    extensions: ["ml", "mli", "mll", "mly"],
    load() {
      return import("./mllike-QM2KT336.js").then((m) => legacy(m.oCaml));
    }
  }),
  LanguageDescription.of({
    name: "Octave",
    extensions: ["m"],
    load() {
      return import("./octave-R22FZGTT.js").then((m) => legacy(m.octave));
    }
  }),
  LanguageDescription.of({
    name: "Oz",
    extensions: ["oz"],
    load() {
      return import("./oz-O4XPDR6P.js").then((m) => legacy(m.oz));
    }
  }),
  LanguageDescription.of({
    name: "Pascal",
    extensions: ["p", "pas"],
    load() {
      return import("./pascal-YWEQUJ44.js").then((m) => legacy(m.pascal));
    }
  }),
  LanguageDescription.of({
    name: "Perl",
    extensions: ["pl", "pm"],
    load() {
      return import("./perl-UDPDYIDF.js").then((m) => legacy(m.perl));
    }
  }),
  LanguageDescription.of({
    name: "Pig",
    extensions: ["pig"],
    load() {
      return import("./pig-X3CVCI26.js").then((m) => legacy(m.pig));
    }
  }),
  LanguageDescription.of({
    name: "PowerShell",
    extensions: ["ps1", "psd1", "psm1"],
    load() {
      return import("./powershell-V7VNTCS6.js").then((m) => legacy(m.powerShell));
    }
  }),
  LanguageDescription.of({
    name: "Properties files",
    alias: ["ini", "properties"],
    extensions: ["properties", "ini", "in"],
    load() {
      return import("./properties-3EDGGWGG.js").then((m) => legacy(m.properties));
    }
  }),
  LanguageDescription.of({
    name: "ProtoBuf",
    extensions: ["proto"],
    load() {
      return import("./protobuf-II5GKJVU.js").then((m) => legacy(m.protobuf));
    }
  }),
  LanguageDescription.of({
    name: "Pug",
    alias: ["jade"],
    extensions: ["pug", "jade"],
    load() {
      return import("./pug-GMDIBOWN.js").then((m) => legacy(m.pug));
    }
  }),
  LanguageDescription.of({
    name: "Puppet",
    extensions: ["pp"],
    load() {
      return import("./puppet-KNDFJT3X.js").then((m) => legacy(m.puppet));
    }
  }),
  LanguageDescription.of({
    name: "Q",
    extensions: ["q"],
    load() {
      return import("./q-Q4O26EZU.js").then((m) => legacy(m.q));
    }
  }),
  LanguageDescription.of({
    name: "R",
    alias: ["rscript"],
    extensions: ["r", "R"],
    load() {
      return import("./r-DQABP3LU.js").then((m) => legacy(m.r));
    }
  }),
  LanguageDescription.of({
    name: "RPM Changes",
    load() {
      return import("./rpm-EWJAPCQM.js").then((m) => legacy(m.rpmChanges));
    }
  }),
  LanguageDescription.of({
    name: "RPM Spec",
    extensions: ["spec"],
    load() {
      return import("./rpm-EWJAPCQM.js").then((m) => legacy(m.rpmSpec));
    }
  }),
  LanguageDescription.of({
    name: "Ruby",
    alias: ["jruby", "macruby", "rake", "rb", "rbx"],
    extensions: ["rb"],
    filename: /^(Gemfile|Rakefile)$/,
    load() {
      return import("./ruby-SDVFQMJB.js").then((m) => legacy(m.ruby));
    }
  }),
  LanguageDescription.of({
    name: "SAS",
    extensions: ["sas"],
    load() {
      return import("./sas-5SNEOXFK.js").then((m) => legacy(m.sas));
    }
  }),
  LanguageDescription.of({
    name: "Scala",
    extensions: ["scala"],
    load() {
      return import("./clike-ZGN5GAC6.js").then((m) => legacy(m.scala));
    }
  }),
  LanguageDescription.of({
    name: "Scheme",
    extensions: ["scm", "ss"],
    load() {
      return import("./scheme-CFXYI2V5.js").then((m) => legacy(m.scheme));
    }
  }),
  LanguageDescription.of({
    name: "Shell",
    alias: ["bash", "sh", "zsh"],
    extensions: ["sh", "ksh", "bash"],
    filename: /^PKGBUILD$/,
    load() {
      return import("./shell-DT7CY57U.js").then((m) => legacy(m.shell));
    }
  }),
  LanguageDescription.of({
    name: "Sieve",
    extensions: ["siv", "sieve"],
    load() {
      return import("./sieve-275ZVVX4.js").then((m) => legacy(m.sieve));
    }
  }),
  LanguageDescription.of({
    name: "Smalltalk",
    extensions: ["st"],
    load() {
      return import("./smalltalk-ETL4OGPW.js").then((m) => legacy(m.smalltalk));
    }
  }),
  LanguageDescription.of({
    name: "Solr",
    load() {
      return import("./solr-UQYXK6HN.js").then((m) => legacy(m.solr));
    }
  }),
  LanguageDescription.of({
    name: "SML",
    extensions: ["sml", "sig", "fun", "smackspec"],
    load() {
      return import("./mllike-QM2KT336.js").then((m) => legacy(m.sml));
    }
  }),
  LanguageDescription.of({
    name: "SPARQL",
    alias: ["sparul"],
    extensions: ["rq", "sparql"],
    load() {
      return import("./sparql-WTDZ323B.js").then((m) => legacy(m.sparql));
    }
  }),
  LanguageDescription.of({
    name: "Spreadsheet",
    alias: ["excel", "formula"],
    load() {
      return import("./spreadsheet-R2QKOH7O.js").then((m) => legacy(m.spreadsheet));
    }
  }),
  LanguageDescription.of({
    name: "Squirrel",
    extensions: ["nut"],
    load() {
      return import("./clike-ZGN5GAC6.js").then((m) => legacy(m.squirrel));
    }
  }),
  LanguageDescription.of({
    name: "Stylus",
    extensions: ["styl"],
    load() {
      return import("./stylus-JFUEZLEU.js").then((m) => legacy(m.stylus));
    }
  }),
  LanguageDescription.of({
    name: "Swift",
    extensions: ["swift"],
    load() {
      return import("./swift-KGY6UM4P.js").then((m) => legacy(m.swift));
    }
  }),
  LanguageDescription.of({
    name: "sTeX",
    load() {
      return import("./stex-3PYIWMF6.js").then((m) => legacy(m.stex));
    }
  }),
  LanguageDescription.of({
    name: "LaTeX",
    alias: ["tex"],
    extensions: ["text", "ltx", "tex"],
    load() {
      return import("./stex-3PYIWMF6.js").then((m) => legacy(m.stex));
    }
  }),
  LanguageDescription.of({
    name: "SystemVerilog",
    extensions: ["v", "sv", "svh"],
    load() {
      return import("./verilog-WIRJ3KU5.js").then((m) => legacy(m.verilog));
    }
  }),
  LanguageDescription.of({
    name: "Tcl",
    extensions: ["tcl"],
    load() {
      return import("./tcl-46OWEU6S.js").then((m) => legacy(m.tcl));
    }
  }),
  LanguageDescription.of({
    name: "Textile",
    extensions: ["textile"],
    load() {
      return import("./textile-MIIFPABH.js").then((m) => legacy(m.textile));
    }
  }),
  LanguageDescription.of({
    name: "TiddlyWiki",
    load() {
      return import("./tiddlywiki-COIK4CJD.js").then((m) => legacy(m.tiddlyWiki));
    }
  }),
  LanguageDescription.of({
    name: "Tiki wiki",
    load() {
      return import("./tiki-LVIEU3PO.js").then((m) => legacy(m.tiki));
    }
  }),
  LanguageDescription.of({
    name: "TOML",
    extensions: ["toml"],
    load() {
      return import("./toml-DEALIZU5.js").then((m) => legacy(m.toml));
    }
  }),
  LanguageDescription.of({
    name: "Troff",
    extensions: ["1", "2", "3", "4", "5", "6", "7", "8", "9"],
    load() {
      return import("./troff-356LC5DU.js").then((m) => legacy(m.troff));
    }
  }),
  LanguageDescription.of({
    name: "TTCN",
    extensions: ["ttcn", "ttcn3", "ttcnpp"],
    load() {
      return import("./ttcn-WPYE5RJU.js").then((m) => legacy(m.ttcn));
    }
  }),
  LanguageDescription.of({
    name: "TTCN_CFG",
    extensions: ["cfg"],
    load() {
      return import("./ttcn-cfg-WFJWEOPA.js").then((m) => legacy(m.ttcnCfg));
    }
  }),
  LanguageDescription.of({
    name: "Turtle",
    extensions: ["ttl"],
    load() {
      return import("./turtle-LNDIGQAV.js").then((m) => legacy(m.turtle));
    }
  }),
  LanguageDescription.of({
    name: "Web IDL",
    extensions: ["webidl"],
    load() {
      return import("./webidl-2NFEXXO5.js").then((m) => legacy(m.webIDL));
    }
  }),
  LanguageDescription.of({
    name: "VB.NET",
    extensions: ["vb"],
    load() {
      return import("./vb-F4BN3JAF.js").then((m) => legacy(m.vb));
    }
  }),
  LanguageDescription.of({
    name: "VBScript",
    extensions: ["vbs"],
    load() {
      return import("./vbscript-CSO33LVU.js").then((m) => legacy(m.vbScript));
    }
  }),
  LanguageDescription.of({
    name: "Velocity",
    extensions: ["vtl"],
    load() {
      return import("./velocity-CJN6557J.js").then((m) => legacy(m.velocity));
    }
  }),
  LanguageDescription.of({
    name: "Verilog",
    extensions: ["v"],
    load() {
      return import("./verilog-WIRJ3KU5.js").then((m) => legacy(m.verilog));
    }
  }),
  LanguageDescription.of({
    name: "VHDL",
    extensions: ["vhd", "vhdl"],
    load() {
      return import("./vhdl-VDW2NC7Z.js").then((m) => legacy(m.vhdl));
    }
  }),
  LanguageDescription.of({
    name: "XQuery",
    extensions: ["xy", "xquery", "xq", "xqm", "xqy"],
    load() {
      return import("./xquery-NMEUL5K5.js").then((m) => legacy(m.xQuery));
    }
  }),
  LanguageDescription.of({
    name: "Yacas",
    extensions: ["ys"],
    load() {
      return import("./yacas-EEXJ535K.js").then((m) => legacy(m.yacas));
    }
  }),
  LanguageDescription.of({
    name: "Z80",
    extensions: ["z80"],
    load() {
      return import("./z80-3JWDDMIW.js").then((m) => legacy(m.z80));
    }
  }),
  LanguageDescription.of({
    name: "MscGen",
    extensions: ["mscgen", "mscin", "msc"],
    load() {
      return import("./mscgen-RGJDNIE2.js").then((m) => legacy(m.mscgen));
    }
  }),
  LanguageDescription.of({
    name: "Xù",
    extensions: ["xu"],
    load() {
      return import("./mscgen-RGJDNIE2.js").then((m) => legacy(m.xu));
    }
  }),
  LanguageDescription.of({
    name: "MsGenny",
    extensions: ["msgenny"],
    load() {
      return import("./mscgen-RGJDNIE2.js").then((m) => legacy(m.msgenny));
    }
  }),
  LanguageDescription.of({
    name: "Vue",
    extensions: ["vue"],
    load() {
      return import("./dist-CZVGTWWO.js").then((m) => m.vue());
    }
  }),
  LanguageDescription.of({
    name: "Angular Template",
    load() {
      return import("./dist-SWI7ZQUC.js").then((m) => m.angular());
    }
  })
];
export {
  languages
};
//# sourceMappingURL=@codemirror_language-data.js.map

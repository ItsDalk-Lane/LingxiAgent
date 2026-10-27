import {
  AbstractMermaidValueConverter,
  EmptyFileSystem,
  MermaidGeneratedSharedModule,
  WardleyGrammarGeneratedModule,
  __name,
  createDefaultCoreModule,
  createDefaultSharedCoreModule,
  inject
} from "./chunk-RY4WWWT5.js";

// node_modules/@mermaid-js/parser/dist/chunks/mermaid-parser.core/chunk-5FCAYU7R.mjs
var _a;
var WardleyValueConverter = (_a = class extends AbstractMermaidValueConverter {
  runCustomConverter(rule, input, _cstNode) {
    switch (rule.name.toUpperCase()) {
      case "LINK_LABEL":
        return input.substring(1).trim();
      default:
        return void 0;
    }
  }
}, __name(_a, "WardleyValueConverter"), _a);
var WardleyModule = {
  parser: {
    ValueConverter: __name(() => new WardleyValueConverter(), "ValueConverter")
  }
};
function createWardleyServices(context = EmptyFileSystem) {
  const shared = inject(
    createDefaultSharedCoreModule(context),
    MermaidGeneratedSharedModule
  );
  const Wardley = inject(
    createDefaultCoreModule({ shared }),
    WardleyGrammarGeneratedModule,
    WardleyModule
  );
  shared.ServiceRegistry.register(Wardley);
  return { shared, Wardley };
}
__name(createWardleyServices, "createWardleyServices");

export {
  WardleyModule,
  createWardleyServices
};
//# sourceMappingURL=chunk-3HT46VZ6.js.map

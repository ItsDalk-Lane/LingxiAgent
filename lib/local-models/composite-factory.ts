import type { LocalModelDescriptor } from "./contracts.ts";
import { LocalModelError } from "./errors.ts";
import type { LocalModelRegistryEntry } from "./registry.ts";
import type { LocalModelInstanceFactory, LocalModelLoadedInstance } from "./runtime-service.ts";

export class CompositeLocalModelInstanceFactory implements LocalModelInstanceFactory {
  private readonly inProcess: LocalModelInstanceFactory;
  private readonly sidecar: LocalModelInstanceFactory;

  constructor(inProcess: LocalModelInstanceFactory, sidecar: LocalModelInstanceFactory) {
    this.inProcess = inProcess;
    this.sidecar = sidecar;
  }

  load(
    descriptor: LocalModelDescriptor,
    installed: LocalModelRegistryEntry,
    signal: AbortSignal,
  ): Promise<LocalModelLoadedInstance> {
    return this.factoryFor(installed).load(descriptor, installed, signal);
  }

  unload(
    instance: LocalModelLoadedInstance,
    descriptor: LocalModelDescriptor,
    signal: AbortSignal,
  ): Promise<void> {
    const kind = instance.diagnostics?.runtimeKind;
    if (kind === "in-process") return this.inProcess.unload(instance, descriptor, signal);
    if (kind === "sidecar") return this.sidecar.unload(instance, descriptor, signal);
    throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "loaded local model has no runtime kind diagnostics");
  }

  async rssMb(instance: LocalModelLoadedInstance): Promise<number> {
    const kind = instance.diagnostics?.runtimeKind;
    const factory = kind === "in-process" ? this.inProcess : kind === "sidecar" ? this.sidecar : null;
    if (!factory?.rssMb) return Number.NaN;
    return Number(await Promise.resolve(factory.rssMb(instance)));
  }

  private factoryFor(installed: LocalModelRegistryEntry): LocalModelInstanceFactory {
    if (installed.runtimeKind === "in-process") return this.inProcess;
    if (installed.runtimeKind === "sidecar") return this.sidecar;
    throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "installed model has an unsupported runtime kind");
  }
}

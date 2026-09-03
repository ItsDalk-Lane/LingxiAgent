import type { LocalModelTier } from "./contracts.ts";
import { LocalModelError } from "./errors.ts";

export interface MemoryReservation {
  key: string;
  tier: LocalModelTier;
  declaredPeakRssMb: number;
  reservedMb: number;
  lastUsedAt: number;
}

export interface MemoryResidentRegistration {
  key: string;
  isInUse: () => boolean;
  evict: () => Promise<void>;
}

export interface MemoryGovernorOptions {
  smallBudgetMb?: number;
  safetyFactor?: number;
  getAvailableMemoryMb: () => number | Promise<number>;
  now?: () => number;
  onEvent?: (event: Record<string, unknown>) => void;
}

export class MemoryGovernor {
  private smallBudgetMb: number;
  private readonly safetyFactor: number;
  private readonly getAvailableMemoryMb: () => number | Promise<number>;
  private readonly now: () => number;
  private readonly onEvent: (event: Record<string, unknown>) => void;
  private readonly reservations = new Map<string, MemoryReservation>();
  private readonly residents = new Map<string, MemoryResidentRegistration>();

  constructor(options: MemoryGovernorOptions) {
    this.smallBudgetMb = options.smallBudgetMb ?? 1536;
    this.safetyFactor = options.safetyFactor ?? 1.25;
    this.getAvailableMemoryMb = options.getAvailableMemoryMb;
    this.now = options.now ?? Date.now;
    this.onEvent = options.onEvent ?? (() => {});
  }

  setSmallBudgetMb(value: number): void {
    if (!Number.isFinite(value) || value <= 0) {
      throw new Error("MemoryGovernor requires a positive smallBudgetMb");
    }
    this.smallBudgetMb = value;
    this.onEvent({ type: "local_model_memory_budget_changed", smallBudgetMb: value });
  }

  getSmallBudgetMb(): number {
    return this.smallBudgetMb;
  }

  async reserve(input: { key: string; tier: LocalModelTier; estimatedPeakRssMb: number }): Promise<MemoryReservation> {
    const existing = this.reservations.get(input.key);
    if (existing) {
      existing.lastUsedAt = this.now();
      return existing;
    }
    if (!Number.isFinite(input.estimatedPeakRssMb) || input.estimatedPeakRssMb <= 0) {
      throw new Error("MemoryGovernor requires a positive estimatedPeakRssMb");
    }
    const reservedMb = Math.ceil(input.estimatedPeakRssMb * this.safetyFactor);
    if (input.tier === "small") {
      const smallReservedMb = [...this.reservations.values()]
        .filter((entry) => entry.tier === "small")
        .reduce((sum, entry) => sum + entry.reservedMb, 0);
      if (smallReservedMb + reservedMb > this.smallBudgetMb) {
        throw insufficient(input.key, reservedMb, this.smallBudgetMb - smallReservedMb, "small_budget");
      }
    }
    const availableMb = await this.getAvailableMemoryMb();
    if (!Number.isFinite(availableMb) || availableMb < reservedMb) {
      throw insufficient(input.key, reservedMb, availableMb, "system_available");
    }
    const reservation = {
      key: input.key,
      tier: input.tier,
      declaredPeakRssMb: input.estimatedPeakRssMb,
      reservedMb,
      lastUsedAt: this.now(),
    };
    this.reservations.set(input.key, reservation);
    this.onEvent({ type: "local_model_memory_reserved", ...reservation });
    return reservation;
  }

  registerResident(registration: MemoryResidentRegistration): void {
    if (!this.reservations.has(registration.key)) {
      throw new Error(`cannot register unreserved local model ${registration.key}`);
    }
    this.residents.set(registration.key, registration);
  }

  touch(key: string): void {
    const entry = this.reservations.get(key);
    if (entry) entry.lastUsedAt = this.now();
  }

  release(key: string): void {
    const reservation = this.reservations.get(key);
    this.residents.delete(key);
    this.reservations.delete(key);
    if (reservation) this.onEvent({ type: "local_model_memory_released", key });
  }

  snapshot(): ReadonlyArray<Readonly<MemoryReservation>> {
    return Object.freeze([...this.reservations.values()].map((entry) => Object.freeze({ ...entry })));
  }

  /** 高内存压力时先逐出最久未用的 large，再逐出最久未用的 small。 */
  async handlePressure(level: "moderate" | "critical"): Promise<string[]> {
    const candidates = [...this.reservations.values()]
      .filter((entry) => {
        const resident = this.residents.get(entry.key);
        return resident && !resident.isInUse();
      })
      .sort((left, right) =>
        (left.tier === right.tier ? 0 : left.tier === "large" ? -1 : 1)
        || left.lastUsedAt - right.lastUsedAt);
    const evicted: string[] = [];
    const targetCount = level === "critical" ? candidates.length : Math.min(1, candidates.length);
    for (const entry of candidates.slice(0, targetCount)) {
      const resident = this.residents.get(entry.key);
      if (!resident || resident.isInUse()) continue;
      await resident.evict();
      evicted.push(entry.key);
      this.onEvent({ type: "local_model_memory_pressure_eviction", level, key: entry.key });
    }
    return evicted;
  }
}

function insufficient(key: string, requiredMb: number, availableMb: number, reason: string): LocalModelError {
  return new LocalModelError(
    "LOCAL_MODEL_MEMORY_INSUFFICIENT",
    `not enough memory to load ${key}; try a smaller quantization or unload another model`,
    { key, requiredMb, availableMb, reason },
  );
}

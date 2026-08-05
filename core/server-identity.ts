import fs from "fs";
import path from "path";
import crypto from "crypto";
import { atomicWriteSync } from "../shared/safe-fs.ts";
import { ensureDeviceAccessRegistries } from "./device-registry.ts";
import {
  ensureExecutionLeaseRegistry,
  EXECUTION_LEASES_FILE,
  executionLeaseRegistryPath,
} from "./execution-lease-registry.ts";
import { ensureGrantRegistry, GRANTS_FILE, grantRegistryPath } from "./grant-registry.ts";
import { SECURITY_DIR } from "./security-dir.ts";
import { ensureServerNetworkConfig } from "./server-network-config.ts";
import { ensureStudioMountRegistry } from "./studio-mounts.ts";

const SERVER_NODE_FILE = "server-node.json";
const USERS_FILE = "users.json";
const STUDIOS_FILE = "studios.json";

export function loadServerIdentity(lingxiHome) {
  const serverNode = readRequiredIdentityJson(path.join(lingxiHome, SERVER_NODE_FILE), SERVER_NODE_FILE);
  const users = readRequiredIdentityJson(path.join(lingxiHome, USERS_FILE), USERS_FILE);
  const studios = readRequiredIdentityJson(path.join(lingxiHome, STUDIOS_FILE), STUDIOS_FILE);

  validateServerNodeIdentity(serverNode, SERVER_NODE_FILE);
  validateUsersIdentity(users, USERS_FILE);
  validateStudiosIdentity(studios, STUDIOS_FILE);
  validateIdentityRegistryLinks(users, studios);

  const defaultUser = users.users.find((user) => user.userId === users.defaultUserId);
  const defaultStudio = getDefaultStudio(studios);
  const serverNodeScope = toServerNodeScope(serverNode);

  return {
    serverId: serverNode.serverId,
    ...serverNodeScope,
    userId: defaultUser.userId,
    studioId: defaultStudio.studioId,
    label: serverNode.label,
    userLabel: defaultUser.displayName,
    studioLabel: defaultStudio.label,
    userKind: defaultUser.kind,
    studioKind: defaultStudio.kind,
    membershipModel: defaultStudio.membershipModel,
    storage: defaultStudio.storage || null,
  };
}

export function ensureLocalIdentityRegistries(lingxiHome) {
  const serverNodePath = path.join(lingxiHome, SERVER_NODE_FILE);
  const usersPath = path.join(lingxiHome, USERS_FILE);
  const studiosPath = path.join(lingxiHome, STUDIOS_FILE);

  const existingServerNode = readIdentityJsonIfPresent(serverNodePath, SERVER_NODE_FILE);
  const existingUsers = readIdentityJsonIfPresent(usersPath, USERS_FILE);
  const existingStudios = readIdentityJsonIfPresent(studiosPath, STUDIOS_FILE);

  if (existingServerNode) validateServerNodeIdentity(existingServerNode, SERVER_NODE_FILE);
  if (existingUsers) validateUsersIdentity(existingUsers, USERS_FILE);
  if (existingStudios) validateStudiosIdentity(existingStudios, STUDIOS_FILE);

  const now = new Date().toISOString();
  const users = existingUsers || createDefaultUsersIdentity({ now });
  const studios = existingStudios || createDefaultStudiosIdentity({
    ownerUserId: users.defaultUserId,
    now,
  });
  const serverNode = existingServerNode || createLocalServerNodeIdentity({ now });
  const repairedIdentityLinks = repairLocalIdentityRegistryLinks(users, studios, { now });

  validateIdentityRegistryLinks(users, studios);

  if (!existingServerNode) writeJsonAtomic(serverNodePath, serverNode);
  if (!existingUsers) writeJsonAtomic(usersPath, users);
  else if (repairedIdentityLinks.users) writeJsonAtomic(usersPath, users);
  if (!existingStudios) writeJsonAtomic(studiosPath, studios);
  else if (repairedIdentityLinks.studios) writeJsonAtomic(studiosPath, studios);

  const foundationRegistries = ensureRemoteAccessFoundationRegistries(lingxiHome, { now });

  return {
    created: [
      !existingServerNode ? SERVER_NODE_FILE : null,
      !existingUsers ? USERS_FILE : null,
      !existingStudios ? STUDIOS_FILE : null,
      ...foundationRegistries.created,
    ].filter(Boolean),
  };
}

function repairLocalIdentityRegistryLinks(users, studios, { now }) {
  const userIds = new Set(users.users.map((user) => user.userId));
  const defaultStudio = getDefaultStudio(studios);
  const repaired = { users: false, studios: false };

  if (userIds.has(defaultStudio.ownerUserId)) {
    if (users.defaultUserId !== defaultStudio.ownerUserId) {
      users.defaultUserId = defaultStudio.ownerUserId;
      users.updatedAt = now;
      repaired.users = true;
    }
    return repaired;
  }

  defaultStudio.ownerUserId = users.defaultUserId;
  defaultStudio.updatedAt = now;
  studios.updatedAt = now;
  repaired.studios = true;
  return repaired;
}

export function ensureRemoteAccessFoundationRegistries(lingxiHome, { now = new Date().toISOString() } = {}) {
  return {
    created: [
      ...ensureDeviceAccessRegistries(lingxiHome, { now }).created,
      ...ensureServerNetworkConfig(lingxiHome, { now }).created,
      ...ensureStudioMountRegistry(lingxiHome, { now }).created,
      ...ensureSecurityRegistries(lingxiHome, { now }).created,
    ],
  };
}

function ensureSecurityRegistries(lingxiHome, { now }) {
  const created = [];
  const grantPath = grantRegistryPath(lingxiHome);
  const leasePath = executionLeaseRegistryPath(lingxiHome);
  const hadGrant = fs.existsSync(grantPath);
  const hadLease = fs.existsSync(leasePath);
  ensureGrantRegistry(lingxiHome, { now });
  ensureExecutionLeaseRegistry(lingxiHome, { now });
  if (!hadGrant) created.push(path.join(SECURITY_DIR, GRANTS_FILE));
  if (!hadLease) created.push(path.join(SECURITY_DIR, EXECUTION_LEASES_FILE));
  return { created };
}

function readRequiredIdentityJson(filePath, label) {
  try {
    return JSON.parse(fs.readFileSync(filePath, "utf-8"));
  } catch (err) {
    if (err.code === "ENOENT") throw new Error(`${label} not found`);
    if (err instanceof SyntaxError) throw new Error(`invalid ${label}: ${err.message}`);
    throw new Error(`failed to read ${label}: ${err.message}`);
  }
}

function readIdentityJsonIfPresent(filePath, label) {
  try {
    return JSON.parse(fs.readFileSync(filePath, "utf-8"));
  } catch (err) {
    if (err.code === "ENOENT") return null;
    if (err instanceof SyntaxError) throw new Error(`invalid ${label}: ${err.message}`);
    throw new Error(`failed to read ${label}: ${err.message}`);
  }
}

function writeJsonAtomic(filePath, data) {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  atomicWriteSync(filePath, JSON.stringify(data, null, 2) + "\n");
}

function createLocalServerNodeIdentity({ now }) {
  const serverId = `server_${crypto.randomUUID()}`;
  return {
    schemaVersion: 1,
    serverId,
    serverNodeId: serverId,
    nodeKind: "local",
    transport: "loopback",
    execution: {
      kind: "local_process",
    },
    label: "Local Hana",
    createdAt: now,
    updatedAt: now,
  };
}

function createDefaultUsersIdentity({ now }) {
  const resolvedUserId = `user_${crypto.randomUUID()}`;
  return {
    schemaVersion: 1,
    defaultUserId: resolvedUserId,
    users: [{
      userId: resolvedUserId,
      kind: "legacy_owner",
      displayName: "Local User",
      profileSource: "legacy_user_profile",
      createdAt: now,
      updatedAt: now,
    }],
    createdAt: now,
    updatedAt: now,
  };
}

function createDefaultStudiosIdentity({ ownerUserId, now }) {
  const studioId = `studio_${crypto.randomUUID()}`;
  return {
    schemaVersion: 1,
    defaultStudioId: studioId,
    studios: [{
      studioId,
      ownerUserId,
      label: "Personal Studio",
      kind: "personal",
      storage: {
        provider: "legacy_hana_home",
        legacyRoot: true,
      },
      membershipModel: "single_user_implicit",
      createdAt: now,
      updatedAt: now,
    }],
    createdAt: now,
    updatedAt: now,
  };
}

function validateServerNodeIdentity(value, label) {
  if (!isPlainObject(value)) throw new Error(`invalid ${label}: expected object`);
  if (value.schemaVersion !== 1) throw new Error(`invalid ${label}: schemaVersion must be 1`);
  if (!isNonEmptyString(value.serverId)) throw new Error(`invalid ${label}: serverId required`);
  if (value.serverNodeId !== undefined && !isNonEmptyString(value.serverNodeId)) {
    throw new Error(`invalid ${label}: serverNodeId must be a non-empty string`);
  }
  if (value.nodeKind !== undefined && !isNonEmptyString(value.nodeKind)) {
    throw new Error(`invalid ${label}: nodeKind must be a non-empty string`);
  }
  if (value.transport !== undefined && !isNonEmptyString(value.transport)) {
    throw new Error(`invalid ${label}: transport must be a non-empty string`);
  }
  if (value.execution !== undefined) {
    if (!isPlainObject(value.execution)) throw new Error(`invalid ${label}: execution must be object`);
    if (value.execution.kind !== undefined && !isNonEmptyString(value.execution.kind)) {
      throw new Error(`invalid ${label}: execution.kind must be a non-empty string`);
    }
  }
  if (!isNonEmptyString(value.label)) throw new Error(`invalid ${label}: label required`);
}

function validateUsersIdentity(value, label) {
  if (!isPlainObject(value)) throw new Error(`invalid ${label}: expected object`);
  if (value.schemaVersion !== 1) throw new Error(`invalid ${label}: schemaVersion must be 1`);
  if (!isNonEmptyString(value.defaultUserId)) throw new Error(`invalid ${label}: defaultUserId required`);
  if (!Array.isArray(value.users) || value.users.length === 0) {
    throw new Error(`invalid ${label}: users must be a non-empty array`);
  }
  const seen = new Set();
  for (const user of value.users) {
    if (!isPlainObject(user)) throw new Error(`invalid ${label}: user must be object`);
    if (!isNonEmptyString(user.userId)) throw new Error(`invalid ${label}: userId required`);
    if (seen.has(user.userId)) throw new Error(`invalid ${label}: duplicate userId ${user.userId}`);
    seen.add(user.userId);
    if (!isNonEmptyString(user.kind)) throw new Error(`invalid ${label}: user.kind required`);
    if (!isNonEmptyString(user.displayName)) throw new Error(`invalid ${label}: user.displayName required`);
  }
  if (!seen.has(value.defaultUserId)) {
    throw new Error(`invalid ${label}: defaultUserId must reference an existing user`);
  }
}

function validateStudiosIdentity(value, label) {
  if (!isPlainObject(value)) throw new Error(`invalid ${label}: expected object`);
  if (value.schemaVersion !== 1) throw new Error(`invalid ${label}: schemaVersion must be 1`);
  if (!isNonEmptyString(value.defaultStudioId)) throw new Error(`invalid ${label}: defaultStudioId required`);
  if (!Array.isArray(value.studios) || value.studios.length === 0) {
    throw new Error(`invalid ${label}: studios must be a non-empty array`);
  }
  const seen = new Set();
  for (const studio of value.studios) {
    if (!isPlainObject(studio)) throw new Error(`invalid ${label}: studio must be object`);
    if (!isNonEmptyString(studio.studioId)) throw new Error(`invalid ${label}: studioId required`);
    if (seen.has(studio.studioId)) throw new Error(`invalid ${label}: duplicate studioId ${studio.studioId}`);
    seen.add(studio.studioId);
    if (!isNonEmptyString(studio.ownerUserId)) throw new Error(`invalid ${label}: ownerUserId required`);
    if (!isNonEmptyString(studio.label)) throw new Error(`invalid ${label}: studio.label required`);
    if (!isNonEmptyString(studio.kind)) throw new Error(`invalid ${label}: studio.kind required`);
    if (!isNonEmptyString(studio.membershipModel)) throw new Error(`invalid ${label}: membershipModel required`);
  }
  if (!seen.has(value.defaultStudioId)) {
    throw new Error(`invalid ${label}: defaultStudioId must reference an existing studio`);
  }
}

function validateIdentityRegistryLinks(users, studios) {
  const userIds = new Set(users.users.map((user) => user.userId));
  const defaultStudio = getDefaultStudio(studios);
  if (!userIds.has(defaultStudio.ownerUserId)) {
    throw new Error("invalid identity registries: default Studio ownerUserId must reference an existing user");
  }
  if (defaultStudio.ownerUserId !== users.defaultUserId) {
    throw new Error("invalid identity registries: default Studio ownerUserId must match defaultUserId");
  }
}

function getDefaultStudio(studios) {
  return studios.studios.find((studio) => studio.studioId === studios.defaultStudioId);
}

function toServerNodeScope(serverNode) {
  return {
    serverNodeId: serverNode.serverNodeId || serverNode.serverId,
    serverNodeKind: serverNode.nodeKind || serverNode.kind || "local",
    serverNodeTransport: serverNode.transport || "loopback",
  };
}

function isPlainObject(value) {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function isNonEmptyString(value) {
  return typeof value === "string" && value.trim().length > 0;
}

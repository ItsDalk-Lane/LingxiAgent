/**
 * "fs" 具名导入的委托再导出（仅基准/证据运行环境经 module.registerHooks 按父模块重定向）。
 *
 * 背景：Node 内置模块的 ESM 具名导出在启动时快照，运行时改写 CJS 导出对象只能拦截
 * `import fs from "fs"` 这类属性访问调用者；SDK（@earendil-works/pi-coding-agent）与仓库
 * 部分模块使用 `import { readSync } from "fs"` 具名导入，必须经解析重定向到本模块，
 * 让具名绑定在每次调用时转发到 fs 默认导出对象（计数器 install 后即是被包装版本）。
 *
 * 本模块自身不做计数——计数在 history-read-counters.mjs 的包装层完成，避免双重计数。
 */

import fsDefault from "node:fs";

export default fsDefault;
export * from "node:fs";

export const openSync = (...args) => fsDefault.openSync(...args);
export const readSync = (...args) => fsDefault.readSync(...args);
export const closeSync = (...args) => fsDefault.closeSync(...args);
export const createReadStream = (...args) => fsDefault.createReadStream(...args);
export const writeFileSync = (...args) => fsDefault.writeFileSync(...args);
export const appendFileSync = (...args) => fsDefault.appendFileSync(...args);
export const readFileSync = (...args) => fsDefault.readFileSync(...args);
export const existsSync = (...args) => fsDefault.existsSync(...args);
export const statSync = (...args) => fsDefault.statSync(...args);
export const mkdirSync = (...args) => fsDefault.mkdirSync(...args);
export const readdirSync = (...args) => fsDefault.readdirSync(...args);

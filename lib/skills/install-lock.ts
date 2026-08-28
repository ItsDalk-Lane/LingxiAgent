/**
 * 技能池变更互斥锁
 *
 * 原先的 install lock 是 createSkillsRoute 闭包里的私有队列，只保护 skills
 * 路由内部的安装/删除/reload。「删除助手时连带删除技能」发生在 agents 路由，
 * 同样要和技能安装/reload 互斥，抽成模块级单例让两个路由共享同一个队列。
 */
let _lock: Promise<unknown> = Promise.resolve();

export function withSkillMutationLock<T>(fn: () => Promise<T>): Promise<T> {
  const prev = _lock;
  let release: () => void;
  _lock = new Promise<void>(resolve => { release = resolve; });
  return prev.then(fn).finally(() => release());
}

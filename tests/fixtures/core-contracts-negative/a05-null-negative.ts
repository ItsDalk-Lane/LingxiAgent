// P01-A05 负例：访问可能为 null 的值。
// 本文件是 strict 门禁的故意违规样本：注入 typecheck:core-contracts 时必须
// 产生空值检查类错误（TS2531 等）。被 tsconfig.test.json 显式排除，永不进入
// 常规工程；只有 tests/core-contracts-strict.test.ts 以 extraFiles 显式注入。
declare function maybeConfig(): { home: string } | null;

export const negativeHome: string = maybeConfig().home;

const { notarize } = require('@electron/notarize');
// electron-builder 的 afterSign 在 26.8.x 只接受单个 string/function，不支持数组。
// 因此 notarize.cjs 作为唯一 afterSign 入口，在公证之前先调用 resignAdhoc 统一签名身份，
// 再按需公证。由 resignAdhoc 内部判断「有 CSC_LINK 则跳过」（让 Developer ID 真签名生效）。
const { resignAdhoc } = require('./resign-adhoc.cjs');

exports.default = async function notarizing(context) {
  const { electronPlatformName, appOutDir } = context;
  if (electronPlatformName !== 'darwin') return;

  // 先重签：无 Apple 证书时把包内所有 Mach-O 统一为 ad-hoc（消除 Team ID 不一致，
  // 修复 v0.1.0 macOS 启动即崩溃）；有证书时 resignAdhoc 内部会自动跳过。
  await resignAdhoc(context);

  if (process.env.SKIP_NOTARIZE === 'true') {
    console.log('Skipping notarization (SKIP_NOTARIZE=true)');
    return;
  }

  const appName = context.packager.appInfo.productFilename;
  console.log(`Notarizing ${appName}...`);

  const password = process.env.APPLE_APP_SPECIFIC_PASSWORD || process.env.APPLE_ID_PASSWORD;
  if (!password) {
    throw new Error('Set APPLE_APP_SPECIFIC_PASSWORD or APPLE_ID_PASSWORD for notarization');
  }

  await notarize({
    appPath: `${appOutDir}/${appName}.app`,
    appleId: process.env.APPLE_ID,
    appleIdPassword: password,
    teamId: process.env.APPLE_TEAM_ID,
  });

  console.log('Notarization complete.');
};


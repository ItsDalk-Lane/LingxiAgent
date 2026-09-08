/**
 * speech_permissions.mm — 宿主 Speech 授权 Node-API 桥（Electron 主进程加载）。
 *
 * 导出：
 *   getAuthorizationStatus(): number   — SFSpeechRecognizer.authorizationStatus rawValue
 *   requestAuthorization(cb): void     — +[SFSpeechRecognizer requestAuthorization:]，
 *                                        回调经 TSFN 跨线程投递；同一时刻最多一个在途
 *   dispose(): void                    — 释放 TSFN；晚到的原生回调丢弃
 *
 * 为什么在宿主进程做：裸 CLI 子进程请求 Speech 授权会被 TCC 直接 SIGABRT（实测），
 * 授权必须由带 Info.plist 使用说明的宿主 App（Lingxi.app）发起。
 */

#import <Foundation/Foundation.h>
#import <Speech/Speech.h>

#import <node_api.h>

#include <mutex>

namespace {

std::mutex g_mutex;
napi_threadsafe_function g_tsfn = nullptr;

struct StatusPayload {
  int raw;
};

// TSFN 的 JS 侧回调：把 rawValue 作为唯一参数调 JS callback，然后一次性释放 TSFN。
void CallJs(napi_env env, napi_value js_callback, void* /*context*/, void* data) {
  StatusPayload* payload = static_cast<StatusPayload*>(data);
  const int raw = payload ? payload->raw : -1;
  delete payload;

  if (env != nullptr && js_callback != nullptr) {
    napi_value argv[1];
    napi_create_int32(env, raw, &argv[0]);
    napi_value undefined;
    napi_get_undefined(env, &undefined);
    napi_value result;
    // 回调里抛错不影响原生侧（TSFN 投递语义不携带返回值）。
    napi_call_function(env, undefined, js_callback, 1, argv, &result);
  }

  std::lock_guard<std::mutex> lock(g_mutex);
  if (g_tsfn != nullptr) {
    napi_release_threadsafe_function(g_tsfn, napi_tsfn_release);
    g_tsfn = nullptr;
  }
}

void FinalizeTsfn(napi_env /*env*/, void* /*finalize_data*/, void* /*finalize_hint*/) {
  // TSFN 生命周期结束（release 或 abort）后的清理点；状态由 g_mutex 管理。
}

napi_value GetAuthorizationStatus(napi_env env, napi_callback_info /*info*/) {
  napi_value result;
  napi_create_int32(env, static_cast<int32_t>([SFSpeechRecognizer authorizationStatus]), &result);
  return result;
}

napi_value RequestAuthorization(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1) {
    napi_throw_type_error(env, nullptr, "requestAuthorization requires a callback");
    return nullptr;
  }
  napi_valuetype arg_type;
  napi_typeof(env, argv[0], &arg_type);
  if (arg_type != napi_function) {
    napi_throw_type_error(env, nullptr, "requestAuthorization callback must be a function");
    return nullptr;
  }

  {
    std::lock_guard<std::mutex> lock(g_mutex);
    if (g_tsfn != nullptr) {
      napi_throw_error(env, nullptr, "speech authorization request already in flight");
      return nullptr;
    }
  }

  napi_value resource_name;
  napi_create_string_utf8(env, "lingxi-speech-permissions", NAPI_AUTO_LENGTH, &resource_name);

  napi_threadsafe_function tsfn = nullptr;
  // 初始线程计数 0：线程使用权完全由 CallJs 的一次性 release 管理。
  napi_status status = napi_create_threadsafe_function(
      env,
      argv[0],
      nullptr,
      resource_name,
      0,
      1,
      nullptr,
      FinalizeTsfn,
      nullptr,
      CallJs,
      &tsfn);
  if (status != napi_ok || tsfn == nullptr) {
    napi_throw_error(env, nullptr, "failed to create threadsafe function");
    return nullptr;
  }

  {
    std::lock_guard<std::mutex> lock(g_mutex);
    g_tsfn = tsfn;
  }

  [SFSpeechRecognizer requestAuthorization:^(SFSpeechRecognizerAuthorizationStatus authStatus) {
    // 该回调可能落在任意线程：一律经 TSFN 投递回 JS 线程。
    std::lock_guard<std::mutex> lock(g_mutex);
    if (g_tsfn == nullptr) {
      // dispose 之后晚到的原生回调：丢弃。
      return;
    }
    StatusPayload* payload = new StatusPayload{static_cast<int>(authStatus)};
    napi_call_threadsafe_function(g_tsfn, payload, napi_tsfn_blocking);
  }];

  napi_value undefined;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value Dispose(napi_env env, napi_callback_info /*info*/) {
  std::lock_guard<std::mutex> lock(g_mutex);
  if (g_tsfn != nullptr) {
    // abort：排队中的投递直接丢弃，CallJs 不会再被调用。
    napi_release_threadsafe_function(g_tsfn, napi_tsfn_abort);
    g_tsfn = nullptr;
  }
  napi_value undefined;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value Init(napi_env env, napi_value exports) {
  const napi_property_attributes attrs =
      static_cast<napi_property_attributes>(napi_default | napi_enumerable);
  napi_property_descriptor descriptors[] = {
      {"getAuthorizationStatus", nullptr, GetAuthorizationStatus, nullptr, nullptr, nullptr, attrs, nullptr},
      {"requestAuthorization", nullptr, RequestAuthorization, nullptr, nullptr, nullptr, attrs, nullptr},
      {"dispose", nullptr, Dispose, nullptr, nullptr, nullptr, attrs, nullptr},
  };
  napi_define_properties(env, exports, 3, descriptors);
  return exports;
}

}  // namespace

NAPI_MODULE(lingxi_speech_permissions, Init)

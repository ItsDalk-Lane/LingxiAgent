{
  "targets": [
    {
      "target_name": "lingxi-speech-permissions",
      "sources": ["speech_permissions.mm"],
      "defines": ["NAPI_VERSION=8"],
      "xcode_settings": {
        "CLANG_ENABLE_OBJC_ARC": "YES",
        "MACOSX_DEPLOYMENT_TARGET": "14.0",
        "OTHER_LDFLAGS": ["-framework", "Speech", "-framework", "Foundation"]
      }
    }
  ]
}

# Contributing

Issues and pull requests are welcome.

Before opening a pull request:

1. Run tests/test_cu.zsh.
2. Run zsh -n bin/cu.
3. Run scripts/build-native.zsh.
4. Keep real desktop actions out of automated tests; use the fixtures and
   CU_OSASCRIPT, CU_NATIVE, CU_SCREENCAPTURE, CU_SIPS, and CLICLICK overrides.
5. Document any macOS version or permission requirements.
6. Do not include screenshots, accessibility dumps, logs, or credentials.

Changes that affect input delivery should include a short safety note and a
mocked regression test where practical.

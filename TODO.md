# Agent OS - TODO

## v1 - Core Functionality

### Must Have
- [x] Task queue (add, list, get next)
- [x] LLM integration (Ollama)
- [x] Tool execution (get_time, list_directory, read_file, http_get, search_web, execute_command)
- [x] Task processing loop
- [x] HTTP API
- [ ] **Auto-process loop** (run automatically, not manual /process)
- [ ] **Agent persistence** (save/restore from disk)

### Should Have
- [x] TOML config loading
- [ ] Error logging to file
- [ ] Health endpoint

### Nice to Have (v2+)
- [ ] MCP server export
- [ ] Tool permissions
- [ ] Private inference routing
- [ ] MCP client

## Code Quality
- [ ] Add unit tests for core functions
- [ ] Integration tests for API
- [ ] 75% test coverage target

---

**Focus:** Make v1 core work reliably first. Don't add features until persistence + auto-loop are done.

# Development checklist

## Project

- [x] Set up the Rust project
- [x] Write the README and list API endpoints
- [x] Set up the HTTP server on port 58881
- [x] Add CLI and environment settings
- [x] Add test key auth and OpenAI-style errors
- [x] Add fixed test data and model settings

## Core API

- [x] Model list and lookup
- [x] Chat Completions
- [x] Responses
- [ ] Embeddings
- [ ] SSE streams for Chat Completions and Responses
- [ ] Tool calls and JSON output

## Saved data

- [ ] Save, list, get, update, and delete chat replies
- [ ] Save, get, and delete responses; list input items
- [ ] File upload, list, lookup, download, and delete
- [ ] Store rules and list paging

## More APIs

- [ ] Image generation and edits
- [ ] Audio speech, transcription, and translation
- [ ] Moderation results

## Tests and release

- [ ] Error, rate limit, delay, and broken stream cases
- [ ] API and stream tests
- [ ] OpenAI SDK checks
- [ ] Update README examples and setup steps
- [ ] Format, lint, and build checks
- [ ] First release

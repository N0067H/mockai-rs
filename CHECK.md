# Development checklist

## Project

- [x] Set up the Rust project
- [x] Write the README and list API endpoints
- [x] Set up the HTTP server on port 58881
- [x] Add CLI and environment settings
- [ ] Add test key auth and OpenAI-style errors
- [ ] Add fixed test data and model settings

## Core API

- [ ] Model list and lookup
- [ ] Chat Completions
- [ ] Responses
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

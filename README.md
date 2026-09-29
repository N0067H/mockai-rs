# mockai-rs

A local mock server for testing apps that use the OpenAI API. Built with Rust.
Change your client's `base_url` to test fixed replies, streams, tool calls, and errors.
Use test data to get the same result on each run, without real model calls.

## Setup

Run the server:

```sh
cargo run
```

Set the host and port:

```sh
cargo run -- --host 127.0.0.1 --port 58882
```

Or use environment variables:

```sh
MOCKAI_HOST=127.0.0.1 MOCKAI_PORT=58882 cargo run
```

CLI values take priority over environment variables, then defaults.
`--host` takes an IPv4 or IPv6 address. `--port` takes a number from 0 to 65535;
0 lets the OS pick a free port. The server prints the bound address on start.
Use `cargo run -- --help` to see all options.

Check the server at `http://localhost:58881/`. It returns `mockai-rs`.
Model list, model lookup, text Chat Completions, and text Responses are available.
The other API endpoints below are not available yet.

| Item | Value |
| --- | --- |
| Host and port | `127.0.0.1:58881` |
| API base URL | `http://localhost:58881/v1` |
| Set host | `--host` or `MOCKAI_HOST`; CLI takes priority |
| Set port | `--port` or `MOCKAI_PORT`; CLI takes priority |
| Auth header | `Authorization: Bearer <test-key>` |
| Default test key | `mock-api-key` |
| Set test key | `--api-key` or `MOCKAI_API_KEY`; CLI takes priority |
| Set test data | `--data-file` or `MOCKAI_DATA_FILE`; CLI takes priority |
| API format | OpenAI paths, methods, field names, and response shapes |
| Test data | Fixed replies, with no real model calls |

## Test data

The server loads test data once on start. The built-in data has one model,
`mock-model`, with the reply `Hello from mockai-rs!` and the vector `[0.1, 0.2, 0.3]`.
These values are shared by the API handlers as they are added.

Use your own JSON file:

```sh
cargo run -- --data-file fixtures/default.json
MOCKAI_DATA_FILE=fixtures/default.json cargo run
```

File format:

```json
{
  "models": [
    {
      "id": "mock-model",
      "reply": "Hello from mockai-rs!",
      "embedding": [0.1, 0.2, 0.3]
    }
  ]
}
```

A custom file replaces the built-in data. Add more entries to use more models,
including OpenAI model names. Each model needs a unique ID, a non-empty reply,
and a non-empty vector of finite numbers. IDs use ASCII text with no spaces or slashes.
Unknown fields and bad data stop the server at startup. Restart to load file changes.

Models can also set `created` (Unix time in seconds) and `owned_by`.
Their defaults are `0` and `mockai-rs`. Model API replies include only
`id`, `object`, `created`, and `owned_by`; test replies and vectors stay private.

## Auth and errors

Requests to `/v1` and `/v1/*` need a Bearer test key:

```sh
curl http://localhost:58881/v1/models \
  -H 'Authorization: Bearer mock-api-key'
```

Set a custom key with `cargo run -- --api-key my-test-key` or
`MOCKAI_API_KEY=my-test-key cargo run`. Keys must be non-empty ASCII text with no spaces.
The root path `/` does not need a key.

A missing or bad key returns 401 with code `invalid_api_key`.
With a valid key, API paths not yet added return 404.
Unknown paths and wrong methods return JSON errors with status 404 and 405.
Each error has `message`, `type`, `param`, and `code` inside an `error` object.

## Endpoints

The API uses OpenAI paths and data formats.

### Models and text

| Method | Path | Use |
| --- | --- | --- |
| GET | `/v1/models` | List test models |
| GET | `/v1/models/{model}` | Get a test model; return 404 if missing |
| POST | `/v1/chat/completions` | Return text, a stream, tool calls, or JSON |
| POST | `/v1/responses` | Return text, stream events, tool calls, or JSON |
| POST | `/v1/embeddings` | Return fixed vectors for one or more inputs |

### Saved replies and files

| Method | Path | Use |
| --- | --- | --- |
| GET | `/v1/responses/{response_id}` | Get a saved response |
| DELETE | `/v1/responses/{response_id}` | Delete a saved response |
| GET | `/v1/responses/{response_id}/input_items` | List input items in pages |
| GET | `/v1/chat/completions` | List saved chat replies |
| GET | `/v1/chat/completions/{completion_id}` | Get a saved chat reply |
| POST | `/v1/chat/completions/{completion_id}` | Update metadata |
| DELETE | `/v1/chat/completions/{completion_id}` | Delete a saved chat reply |
| GET | `/v1/chat/completions/{completion_id}/messages` | List saved messages in pages |
| POST | `/v1/files` | Upload a file and get its ID |
| GET | `/v1/files` | List files |
| GET | `/v1/files/{file_id}` | Get file info |
| GET | `/v1/files/{file_id}/content` | Get file bytes |
| DELETE | `/v1/files/{file_id}` | Delete a file |

Saved data stays in memory and clears when the server restarts.
Each API uses its own `store` rules. Chat replies are saved only with `store: true`.
Missing or unsaved items return 404.

### Images, audio, and safety checks

| Method | Path | Use |
| --- | --- | --- |
| POST | `/v1/images/generations` | Return a test image |
| POST | `/v1/images/edits` | Check the input and return a test image |
| POST | `/v1/audio/speech` | Return test audio in a supported format |
| POST | `/v1/audio/transcriptions` | Return fixed text for an audio file |
| POST | `/v1/audio/translations` | Return fixed English text for an audio file |
| POST | `/v1/moderations` | Return fixed safety flags and scores |

## API format

- JSON requests use `application/json`. File uploads use multipart where needed.
- Chat Completions takes `messages` and returns `choices[].message`.
- Responses takes `input` and returns `output` items. Each API has its own response shape.
- With `stream: true`, the same POST endpoint returns `text/event-stream`.
- Chat streams use `chat.completion.chunk` and end with `data: [DONE]`.
- Response streams use events such as `response.created`, `response.output_text.delta`, and `response.completed`.
- Tool calls return a name, arguments as a JSON string, and a call ID. The client runs the tool.
- JSON output uses `response_format` in Chat Completions and `text.format` in Responses.
- `usage` uses fixed test counts or a repeatable count rule. It is not real model usage.
- Bad input and unsupported features return an error.

## Test cases

Test cases cover normal replies, errors, slow replies, and broken streams.

| Case | Result |
| --- | --- |
| Bad input | HTTP 400, `invalid_request_error` |
| Bad test key | HTTP 401, code `invalid_api_key` |
| Missing model or item | HTTP 404 |
| Rate limit | HTTP 429 with `Retry-After` for retry tests |
| Server error | HTTP 500 or 503 |
| Slow reply | Wait before the reply or between stream chunks |
| Broken stream | Send a few events, then close the connection |

Error example:

```json
{
  "error": {
    "message": "Missing required parameter: 'model'.",
    "type": "invalid_request_error",
    "param": "model",
    "code": null
  }
}
```

## Request examples

List models or get one model:

```sh
curl http://localhost:58881/v1/models \
  -H 'Authorization: Bearer mock-api-key'

curl http://localhost:58881/v1/models/mock-model \
  -H 'Authorization: Bearer mock-api-key'
```

The list uses `object: "list"` and a `data` array in file order.
Each model uses `object: "model"`. An unknown model returns 404 with
`code: "model_not_found"` and `param: "model"`.

Chat Completions returns the selected model's fixed `reply` in
`choices[0].message.content`, with a new `chatcmpl-` ID and the current Unix time.
`usage` counts words split by whitespace; it is a test count, not model token usage.

Messages accept `system`, `developer`, `user`, and `assistant` roles.
Content can be a string or an array of `{"type":"text","text":"..."}` parts.
`temperature` (0 to 2) and `top_p` (0 to 1) are checked but do not change the fixed reply.
Supported optional fields can be omitted or set to null. The current API accepts
`stream: false`, `store: false`, `n: 1`, and `response_format: {"type":"text"}`.
Other options, streaming, tools, JSON output, and saved chat replies return 400.
Bad input returns an OpenAI-style error with the field name in `param`.

```sh
curl http://localhost:58881/v1/chat/completions \
  -H 'Authorization: Bearer mock-api-key' \
  -H 'Content-Type: application/json' \
  -d '{"model":"mock-model","messages":[{"role":"user","content":"Hello"}]}'
```

```sh
curl http://localhost:58881/v1/responses \
  -H 'Authorization: Bearer mock-api-key' \
  -H 'Content-Type: application/json' \
  -d '{"model":"mock-model","input":"Hello","store":false}'
```

Responses accepts a string or an array of text messages in `input`, plus optional
`instructions`. Message roles are `system`, `developer`, `user`, and `assistant`.
Content can be a string or `input_text` parts. Assistant messages also accept
`output_text` parts, so you can add the returned `output` messages to the next input.

The fixed reply is in `output[0].content[0].text`, with `status: "completed"`,
a new `resp_` ID, and `created_at`. `usage` counts words in input and instructions,
then words in the reply. Cached and reasoning token counts are zero.
`temperature` and `top_p` use the same ranges as Chat Completions and do not change the reply.
`text: {"format":{"type":"text"}}` is accepted.

This mock currently defaults to `store: false`, unlike OpenAI's default.
`store: true`, `stream: true`, `background: true`, tools, JSON output, and
non-null `previous_response_id` return 400. Saved responses and streams are separate features.

`mock-model` is a test model ID.

## API docs

See official OpenAI documentation for paths and data formats.

- [Chat Completions](https://developers.openai.com/api/reference/resources/chat/subresources/completions)
- [Responses](https://developers.openai.com/api/reference/resources/responses)
- [Models](https://developers.openai.com/api/reference/resources/models)
- [Embeddings](https://developers.openai.com/api/reference/resources/embeddings)
- [Files](https://developers.openai.com/api/reference/resources/files)
- [Images](https://developers.openai.com/api/reference/resources/images)
- [Audio](https://developers.openai.com/api/reference/resources/audio)
- [Moderations](https://developers.openai.com/api/reference/resources/moderations)
- [Error codes](https://developers.openai.com/api/docs/guides/error-codes)

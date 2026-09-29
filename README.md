# mockai-rs

A local mock server for testing apps that use the OpenAI API. Built with Rust.
Change your client's `base_url` to test fixed replies, streams, tool calls, and errors.
Use test data to get the same result on each run, without real model calls.

## Setup

Run the server:

```sh
cargo run
```

Check the server at `http://localhost:58881/`. It returns `mockai-rs`.
The API endpoints below are not available yet.

| Item | Value |
| --- | --- |
| Host and port | `127.0.0.1:58881` |
| API base URL | `http://localhost:58881/v1` |
| Set port | `--port` or `MOCKAI_PORT`; CLI takes priority |
| Auth header | `Authorization: Bearer <test-key>` |
| Default test key | `mock-api-key` |
| API format | OpenAI paths, methods, field names, and response shapes |
| Test data | Fixed replies, with no real model calls |

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
  -d '{"model":"mock-model","input":"Hello","stream":true}' \
  --no-buffer
```

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

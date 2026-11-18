# Hermes Consult with a real loopback model

One synthetic short-hint Consult reached Grok through a native Hermes 0.22.5 Runner. Five requests returned HTTP 200 and identified the model as `grok-4.7-build`. The Consult and its Runner both ended with `PROTOCOL_INVALID`. The observer exited zero after recording this failure. There was no successful public result to score or cite. Provider-reported usage was 29,323 input tokens and 1,363 output tokens. Their sum matches the product's 30,686 tokens. Billing was unobserved. [Redacted per-request observations](hermes-loopback-r1.json)

The Caller used the real Hermes plugin and a deterministic local responder to register a synthetic offline decision and ask about its fallback. The production daemon saved the hint, and the Runner's origin and fence matched that invocation. The independently started Hermes Runner called the operator's configured loopback model. It could request six scoped context and memory tools. The other three host CLIs were absent. The observer limited the run to six upstream calls and 1,200 output tokens per call. A preliminary allowlist check rejected the native tool declaration before inference. The corrected observer presented only the six permitted tool definitions to Grok. The release receipt lacks a field that identifies the exact invalid response. The earlier frozen Codex measurements and the Claude deterministic native checks retain their own model and budget scope. [Host matrix](native-host-matrix-r1.md) · [Isolated single-host workflows](isolated-workflows-r1.md)

## Run one new measurement

The observer needs Python 3 with PyYAML, bubblewrap and the Hermes installation described in the matrix. It accepts an existing operator-selected provider only when its name, chat protocol and exact loopback origin match. It keeps the credential in the outer relay and starts the native host in a private network and filesystem namespace. The relay accepts only the chat-completions route. The command invokes the configured model and consumes its quota. Each output file holds only measurement metadata. The archived observer was replayed against an isolated synthetic server with a deliberately invalid final response. It recorded one local request and the typed product failure without sending a second live model task. [Observer sources](hermes-loopback-observer-r1.tar.gz)

```bash
mkdir -m 700 "$SCRATCH/hermes-observer"
tar -xzf hermes-loopback-observer-r1.tar.gz -C "$SCRATCH/hermes-observer"
python3 -B "$SCRATCH/hermes-observer/run.py" \
  --product "$AKASHA" --scratch "$SCRATCH" --output "$SCRATCH/grok-observation.json" \
  --hermes-owner "$HERMES_OWNER" --hermes-install "$HERMES_INSTALL" \
  --provider-config "$HERMES_CONFIG" --provider-name "$PROVIDER_NAME" \
  --expected-loopback-port "$PROVIDER_PORT"
```

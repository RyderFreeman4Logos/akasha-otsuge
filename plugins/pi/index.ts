import { Type } from '@earendil-works/pi-ai';
import { getAgentDir, VERSION, type ExtensionAPI } from '@earendil-works/pi-coding-agent';
import bridge from './bridge.cjs';

const DEFAULTS = {};

export default function (pi: ExtensionAPI) {
  if (VERSION !== '1.4.0') throw new Error('ADAPTER_UNSUPPORTED');
  if ('AKASHA_PI_RUN_SOCKET' in process.env || 'AKASHA_PI_RUN_TOKEN' in process.env) {
    const cursor = Type.Optional(Type.Union([Type.String(), Type.Null()]));
    const limit = Type.Integer({ minimum: 1, maximum: 16 });
    const object = properties => Type.Object(properties, { additionalProperties: false });
    bridge.registerRunner(pi, getAgentDir, DEFAULTS, VERSION, {
      memory_search: object({ query: Type.String({ maxLength: 512 }) }),
      memory_get: object({ memory_id: Type.String(), revision: Type.Integer({ minimum: 1 }) }),
      code_search: object({ paths: Type.Array(Type.String(), { minItems: 1, maxItems: 16 }), query: Type.String({ maxLength: 512 }) }),
      code_read: object({ path: Type.String(), start: Type.Integer({ minimum: 0 }), max_bytes: Type.Integer({ minimum: 1, maximum: 16384 }) }),
      delegate: object({ hint: Type.String({ maxLength: 512 }) }),
      context_describe: object({}),
      context_list: object({ cursor, limit }),
      context_read: object({ handle: Type.String(), start: Type.Integer({ minimum: 0 }), max_bytes: Type.Integer({ minimum: 1, maximum: 16384 }) }),
      context_search: object({ cursor, query: Type.String(), limit }),
    });
    return;
  }
  bridge.registerCaller(pi, getAgentDir, DEFAULTS, Type.Object({
    hint: Type.Optional(Type.Union([Type.String({ maxLength: 512 }), Type.Null()])),
  }, { additionalProperties: false }));
}

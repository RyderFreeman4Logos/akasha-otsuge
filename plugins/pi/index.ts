import { Type } from '@earendil-works/pi-ai';
import { getAgentDir, VERSION, type ExtensionAPI } from '@earendil-works/pi-coding-agent';
import bridge from './bridge.cjs';

const DEFAULTS = {};

export default function (pi: ExtensionAPI) {
  if (VERSION !== '1.4.0') throw new Error('ADAPTER_UNSUPPORTED');
  bridge.registerCaller(pi, getAgentDir, DEFAULTS, Type.Object({
    hint: Type.Optional(Type.Union([Type.String({ maxLength: 512 }), Type.Null()])),
  }, { additionalProperties: false }));
}

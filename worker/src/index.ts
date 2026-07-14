import { handleRequest } from "./api";
import { collect } from "./collect";
import type { Env } from "./types";

export default {
  async scheduled(event, env, ctx) {
    ctx.waitUntil(collect(env, new Date(event.scheduledTime)));
  },

  async fetch(req, env) {
    return handleRequest(req, env);
  },
} satisfies ExportedHandler<Env>;

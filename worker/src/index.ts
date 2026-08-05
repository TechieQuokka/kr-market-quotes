import { handleRequest } from "./api";
import { collect } from "./collect";
import type { Env } from "./types";

export default {
  async scheduled(event, env, ctx) {
    // 결과를 버리지 않는다 — 실패는 collect_log에 남고, 여기서 한 줄 요약을 로그로도 남긴다.
    ctx.waitUntil(
      collect(env, new Date(event.scheduledTime)).then(
        (r) => {
          if (r.failed.length > 0) {
            console.error(`수집 실패: ${r.failed.join(", ")} (saved=${r.saved})`);
          }
        },
        (e) => console.error("수집 실행 자체가 실패:", e),
      ),
    );
  },

  async fetch(req, env) {
    return handleRequest(req, env);
  },
} satisfies ExportedHandler<Env>;

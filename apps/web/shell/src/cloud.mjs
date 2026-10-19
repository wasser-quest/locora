// The cloud client of the browser. The core runs the cloud commands as steps: each step either
// finishes or asks for one HTTP call. This module sends those calls with fetch and hands the replies
// back to the core, one command at a time, so two cloud commands never share the core's waiting step.

let lane = Promise.resolve();

/**
 * Runs one cloud command to its result. `core` is the instance's call function (see core.mjs), and
 * `persist` saves the core's files after each step (the cloud account is one of them).
 */
export function runCloud(core, persist, command, args) {
  const run = lane.then(() => drive(core, persist, command, args));
  lane = run.catch(() => {});
  return run;
}

async function drive(core, persist, command, args) {
  let reply = core(command, args);
  for (;;) {
    persist();
    if (reply.err !== undefined) throw reply.err;
    if (reply.ok.done !== undefined) return reply.ok.done;
    reply = core.cloudReply(...(await send(core, reply.ok.call)));
  }
}

/** Sends one call. A call that never gets an answer is reported as status 0. */
async function send(core, call) {
  const headers = {};
  if (call.token) headers.authorization = `Bearer ${call.token}`;
  let body;
  if (call.body !== 'none') {
    body = core.cloudBody();
    headers['content-type'] = call.body === 'json' ? 'application/json' : 'application/octet-stream';
  }
  try {
    const response = await fetch(call.url, {
      method: call.method,
      headers,
      body,
      signal: AbortSignal.timeout(call.timeout * 1000),
    });
    return [response.status, new Uint8Array(await response.arrayBuffer())];
  } catch {
    return [0, new Uint8Array(0)];
  }
}

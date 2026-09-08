const express = require("express");
const { Queue } = require("bullmq");
const { createBullBoard } = require("@bull-board/api");
const { BullMQAdapter } = require("@bull-board/api/bullMQAdapter");
const { ExpressAdapter } = require("@bull-board/express");

const queueNames = [
  ...new Set(
    (process.env.BULLMQ_QUEUE_NAMES || "")
      .split(",")
      .map((name) => name.trim())
      .filter(Boolean)
  ),
];

if (queueNames.length === 0) {
  throw new Error("BULLMQ_QUEUE_NAMES en az bir kuyruk adı içermeli.");
}

const queues = queueNames.map((name) => {
  const queue = new Queue(name, {
    connection: {
      host: process.env.REDIS_HOST || "redis",
      port: Number(process.env.REDIS_PORT || 6379),
    },
    prefix: process.env.BULLMQ_PREFIX || "bull",
  });

  queue.on("error", (error) => {
    console.error(`[${name}] Redis error:`, error);
  });

  return queue;
});

const serverAdapter = new ExpressAdapter();
serverAdapter.setBasePath("/");

createBullBoard({
  queues: queues.map(
    (queue) =>
      new BullMQAdapter(queue, {
        readOnlyMode: true,
      })
  ),
  serverAdapter,
});

const app = express();
app.disable("x-powered-by");
app.use("/", serverAdapter.getRouter());

const port = Number(process.env.PORT || 3000);

const server = app.listen(port, "0.0.0.0", () => {
  console.log(`Bull Board listening on port ${port}`);
  console.log(`Queues: ${queueNames.join(", ")}`);
});

let stopping = false;

async function shutdown() {
  if (stopping) return;
  stopping = true;

  server.close(async () => {
    await Promise.allSettled(queues.map((queue) => queue.close()));
    process.exit(0);
  });

  setTimeout(() => process.exit(1), 25000).unref();
}

process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);
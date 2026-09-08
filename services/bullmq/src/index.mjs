import { Queue, Worker } from "bullmq";
import pg from "pg";
import { setTimeout as sleep } from "node:timers/promises";

const { Pool } = pg;

function required(name) {
  const value = process.env[name];

  if (!value) {
    throw new Error(`${name} is required`);
  }

  return value;
}

const config = {
  redisHost: required("REDIS_HOST"),
  redisPort: Number(process.env.REDIS_PORT ?? 6379),
  rustApiUrl: required("RUST_API_URL").replace(/\/$/, ""),
  workerSecret: required("INTERNAL_WORKER_SECRET"),
};

const connection = {
  host: config.redisHost,
  port: config.redisPort,
};

const database = new Pool({
  max: 3,
  connectionTimeoutMillis: 5000,
});

database.on("error", (error) => {
  console.error("PostgreSQL pool error:", error.message);
});

const queueName = "game-jobs";

const queue = new Queue(queueName, {
  connection,
});

queue.on("error", (error) => {
  console.error("Queue error:", error.message);
});

async function executeOnRust(job) {
  const response = await fetch(
    `${config.rustApiUrl}/internal/jobs/${job.data.taskId}/execute`,
    {
      method: "POST",
      headers: {
        "x-worker-secret": config.workerSecret,
      },
      signal: AbortSignal.timeout(15_000),
    },
  );

  if (!response.ok) {
    // Response içeriğini loglamıyoruz; ileride hassas veri içerebilir.
    throw new Error(`Rust job execution returned HTTP ${response.status}`);
  }
}

const worker = new Worker(
  queueName,
  executeOnRust,
  {
    connection,
    concurrency: 5,
  },
);

worker.on("completed", (job) => {
  console.info(`Job completed: ${job.id}`);
});

worker.on("failed", (job, error) => {
  console.error(
    `Job attempt failed: ${job?.id}; ${error.message}`,
  );
});

worker.on("error", (error) => {
  console.error("Worker error:", error.message);
});

async function dispatchPendingJobs() {
  const client = await database.connect();

  try {
    await client.query("BEGIN");

    // Aynı servisten birden fazla instance çalışırsa aynı kayıt için
    // dispatcher yarışını SKIP LOCKED ile önlüyoruz.
    //
    // Tamamlanmamış işleri 30 saniyede bir yeniden kontrol ediyoruz:
    // Redis'te kaybolmuş bir job varsa tekrar oluşturulabilir.
    const result = await client.query(`
      SELECT id, kind, run_at
      FROM scheduled_jobs
      WHERE completed_at IS NULL
        AND (
          last_enqueued_at IS NULL
          OR last_enqueued_at < NOW() - INTERVAL '30 seconds'
        )
      ORDER BY last_enqueued_at ASC NULLS FIRST, run_at ASC
      LIMIT 25
      FOR UPDATE SKIP LOCKED
    `);

    for (const task of result.rows) {
      const delay = Math.max(
        0,
        new Date(task.run_at).getTime() - Date.now(),
      );

      await queue.add(
        task.kind,
        {
          taskId: task.id,
        },
        {
          // UUID içinde BullMQ jobId için yasak olan ":" bulunmaz.
          jobId: task.id,

          delay,

          attempts: 100,
          backoff: {
            type: "fixed",
            delay: 5000,
          },

          // Kullanıcının istediği davranış:
          removeOnComplete: true,

          // Hataları sessizce kaybetme.
          removeOnFail: false,
        },
      );

      await client.query(
        `
        UPDATE scheduled_jobs
        SET last_enqueued_at = NOW()
        WHERE id = $1
        `,
        [task.id],
      );
    }

    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK").catch(() => {});
    throw error;
  } finally {
    client.release();
  }
}

let stopping = false;

async function dispatchLoop() {
  while (!stopping) {
    try {
      await dispatchPendingJobs();
    } catch (error) {
      // İlk açılışta Rust migration'ı henüz çalışmadıysa bekle.
      if (error.code === "42P01") {
        console.info("Waiting for Rust database migrations...");
      } else {
        console.error("Dispatcher error:", error.message);
      }
    }

    await sleep(1000);
  }
}

const dispatcher = dispatchLoop();

async function shutdown(signal) {
  if (stopping) {
    return;
  }

  console.info(`Shutting down: ${signal}`);
  stopping = true;

  try {
    await dispatcher;
    await worker.close();
    await queue.close();
    await database.end();
  } catch (error) {
    console.error("Shutdown error:", error.message);
    process.exitCode = 1;
  }
}

process.once("SIGTERM", () => {
  void shutdown("SIGTERM");
});

process.once("SIGINT", () => {
  void shutdown("SIGINT");
});

console.info("BullMQ dispatcher and worker started");
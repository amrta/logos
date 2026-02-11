const WORKER = 'https://logos-gateway.amrta.workers.dev';
const CONCURRENCY = 20;
const REQUESTS_PER_CLIENT = 50;

async function runClient(id) {
  let success = 0;
  let failed = 0;
  let totalLatency = 0;

  for (let i = 0; i < REQUESTS_PER_CLIENT; i++) {
    const start = Date.now();
    try {
      const response = await fetch(`${WORKER}/status`);
      if (response.ok) success++;
      else failed++;
      totalLatency += (Date.now() - start);
    } catch (_) {
      failed++;
    }
  }

  const n = success + failed;
  return {
    clientId: id,
    success,
    failed,
    avgLatency: n > 0 ? totalLatency / n : 0
  };
}

async function stressTest() {
  console.log(`Stress test: ${CONCURRENCY} clients × ${REQUESTS_PER_CLIENT} requests`);
  const startTime = Date.now();
  const results = await Promise.all(
    Array.from({ length: CONCURRENCY }, (_, i) => runClient(i))
  );
  const endTime = Date.now();

  const totalRequests = results.reduce((s, r) => s + r.success + r.failed, 0);
  const totalSuccess = results.reduce((s, r) => s + r.success, 0);
  const totalFailed = results.reduce((s, r) => s + r.failed, 0);
  const avgLatency = results.reduce((s, r) => s + r.avgLatency, 0) / (results.length || 1);
  const durationSec = (endTime - startTime) / 1000;
  const throughput = totalRequests / durationSec;

  console.log(`
===== Stress Test Results =====
Total Requests: ${totalRequests}
Success: ${totalSuccess} (${((totalSuccess/totalRequests)*100).toFixed(2)}%)
Failed: ${totalFailed}
Duration: ${durationSec.toFixed(2)}s
Throughput: ${throughput.toFixed(2)} req/s
Avg Latency: ${avgLatency.toFixed(2)}ms
================================
  `);

  const successRate = totalSuccess / totalRequests;
  if (successRate >= 0.99 && avgLatency < 200 && throughput >= 50) {
    console.log('✅ All performance targets met!');
  } else {
    console.log('⚠️ Performance targets not met:');
    if (successRate < 0.99) console.log(`  - Success rate ${(successRate*100).toFixed(2)}% < 99%`);
    if (avgLatency >= 200) console.log(`  - Avg latency ${avgLatency.toFixed(2)}ms >= 200ms`);
    if (throughput < 50) console.log(`  - Throughput ${throughput.toFixed(2)} req/s < 50 req/s`);
  }
}

stressTest().catch(console.error);

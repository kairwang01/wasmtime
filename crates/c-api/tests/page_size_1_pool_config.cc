#include <wasmtime.hh>

using namespace wasmtime;

int main() {
  PoolAllocationConfig pooling;
  pooling.total_component_instances(1);
  pooling.total_memories(1);
  pooling.max_memory_size(65536);
  pooling.page_size_1_memory_max_size(64);
  pooling.max_page_size_1_memories_per_component(2);

  Config config;
  config.wasm_custom_page_sizes(true);
  config.memory_reservation(65536);
  config.memory_guard_size(0);
  config.pooling_allocation_strategy(pooling);

  return 0;
}

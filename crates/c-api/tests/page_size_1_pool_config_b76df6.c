#include <wasmtime.h>

int main(void) {
  wasm_config_t *config = wasm_config_new();
  wasmtime_pooling_allocation_config_t *pooling =
      wasmtime_pooling_allocation_config_new();
  if (config == NULL || pooling == NULL) {
    if (pooling != NULL) {
      wasmtime_pooling_allocation_config_delete(pooling);
    }
    if (config != NULL) {
      wasm_config_delete(config);
    }
    return 1;
  }

  wasmtime_pooling_allocation_config_total_component_instances_set(pooling, 1);
  wasmtime_pooling_allocation_config_total_memories_set(pooling, 1);
  wasmtime_pooling_allocation_config_max_memory_size_set(pooling, 65536);
  wasmtime_pooling_allocation_config_page_size_1_memory_max_size_set(pooling,
                                                                    64);
  wasmtime_pooling_allocation_config_max_page_size_1_memories_per_component_set(
      pooling, 2);
  wasmtime_config_wasm_custom_page_sizes_set(config, true);
  wasmtime_config_memory_reservation_set(config, 65536);
  wasmtime_config_memory_guard_size_set(config, 0);
  wasmtime_pooling_allocation_strategy_set(config, pooling);
  wasmtime_pooling_allocation_config_delete(pooling);
  wasm_config_delete(config);
  return 0;
}

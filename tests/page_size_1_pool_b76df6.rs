use clap::Parser;
use wasmtime::component::{Component, Linker as ComponentLinker};
use wasmtime::{
    Config, Engine, Instance, Memory, MemoryType, MemoryTypeBuilder, Module,
    PoolingAllocationConfig, Result, Store,
};
use wasmtime_cli_flags::CommonOptions;

fn small_pool() -> PoolingAllocationConfig {
    let mut pool = PoolingAllocationConfig::new();
    pool.total_component_instances(1)
        .total_core_instances(16)
        .total_memories(1)
        .max_memory_size(1 << 16)
        .total_tables(1)
        .table_elements(10)
        .total_stacks(1);
    pool
}

fn pooling_engine(pool: PoolingAllocationConfig) -> Result<Engine> {
    let mut config = Config::new();
    config
        .wasm_custom_page_sizes(true)
        .wasm_component_model(true)
        .wasm_multi_memory(true)
        .memory_reservation(1 << 16)
        .memory_reservation_for_growth(0)
        .memory_guard_size(0)
        .allocation_strategy(pool);
    Engine::new(&config)
}

fn live_instance(engine: &Engine, module: &Module) -> Result<Store<()>> {
    let mut store = Store::new(engine, ());
    Instance::new(&mut store, module, &[])?;
    Ok(store)
}

fn instance_with_handle(engine: &Engine, module: &Module) -> Result<(Store<()>, Instance)> {
    let mut store = Store::new(engine, ());
    let instance = Instance::new(&mut store, module, &[])?;
    Ok((store, instance))
}

fn instance_is_rejected(engine: &Engine, module: &Module) -> bool {
    let mut store = Store::new(engine, ());
    Instance::new(&mut store, module, &[]).is_err()
}

fn module_is_rejected(engine: &Engine, source: &str) -> bool {
    let module = match Module::new(engine, source) {
        Ok(module) => module,
        Err(_) => return true,
    };
    instance_is_rejected(engine, &module)
}

fn component_succeeds(engine: &Engine, source: &str) -> Result<()> {
    let component = Component::new(engine, source)?;
    let linker = ComponentLinker::<()>::new(engine);
    let mut store = Store::new(engine, ());
    linker.instantiate(&mut store, &component)?;
    Ok(())
}

fn component_is_rejected(engine: &Engine, source: &str) -> bool {
    let component = match Component::new(engine, source) {
        Ok(component) => component,
        Err(_) => return true,
    };
    let linker = ComponentLinker::<()>::new(engine);
    let mut store = Store::new(engine, ());
    linker.instantiate(&mut store, &component).is_err()
}

#[test]
fn page_size_1_pool_configuration() -> Result<()> {
    let disabled = small_pool();
    assert_eq!(disabled.get_page_size_1_memory_max_size(), 0);
    assert_eq!(disabled.get_max_page_size_1_memories_per_component(), 0);
    pooling_engine(disabled)?;

    let mut only_size = small_pool();
    only_size.page_size_1_memory_max_size(32);
    assert!(pooling_engine(only_size).is_err());

    let mut only_count = small_pool();
    only_count.max_page_size_1_memories_per_component(1);
    assert!(pooling_engine(only_count).is_err());

    let mut enabled = small_pool();
    enabled
        .page_size_1_memory_max_size(32)
        .max_page_size_1_memories_per_component(2);
    assert_eq!(enabled.get_page_size_1_memory_max_size(), 32);
    assert_eq!(enabled.get_max_page_size_1_memories_per_component(), 2);
    pooling_engine(enabled)?;
    Ok(())
}

#[test]
fn page_size_1_pool_validates_capacity_edges() -> Result<()> {
    let mut overflow = small_pool();
    overflow
        .total_component_instances(u32::MAX)
        .page_size_1_memory_max_size(1)
        .max_page_size_1_memories_per_component(2);
    assert!(pooling_engine(overflow).is_err());

    let mut zero_capacity = small_pool();
    zero_capacity
        .total_component_instances(0)
        .page_size_1_memory_max_size(32)
        .max_page_size_1_memories_per_component(1);
    let engine = pooling_engine(zero_capacity)?;
    let custom = Module::new(&engine, "(module (memory 1 32 (pagesize 1)))")?;
    let ordinary = Module::new(&engine, "(module (memory 1 1))")?;

    assert!(instance_is_rejected(&engine, &custom));
    live_instance(&engine, &ordinary)?;
    Ok(())
}

#[test]
fn page_size_1_pool_falls_back_to_shared_capacity() -> Result<()> {
    let engine = pooling_engine(small_pool())?;
    let custom = Module::new(&engine, "(module (memory 1 32 (pagesize 1)))")?;
    let ordinary = Module::new(&engine, "(module (memory 1 1))")?;

    let custom_store = live_instance(&engine, &custom)?;
    assert!(instance_is_rejected(&engine, &ordinary));
    drop(custom_store);

    let ordinary_store = live_instance(&engine, &ordinary)?;
    assert!(instance_is_rejected(&engine, &custom));
    drop(ordinary_store);

    live_instance(&engine, &custom)?;
    Ok(())
}

#[test]
fn page_size_1_pool_capacities_are_independent() -> Result<()> {
    let mut pool = small_pool();
    pool.total_component_instances(2)
        .total_memories(1)
        .page_size_1_memory_max_size(32)
        .max_page_size_1_memories_per_component(2);
    let engine = pooling_engine(pool)?;
    let custom = Module::new(&engine, "(module (memory 1 32 (pagesize 1)))")?;
    let ordinary = Module::new(&engine, "(module (memory 1 1))")?;

    let mut custom_stores = Vec::new();
    for _ in 0..4 {
        custom_stores.push(live_instance(&engine, &custom)?);
    }
    let ordinary_store = live_instance(&engine, &ordinary)?;

    assert!(instance_is_rejected(&engine, &custom));
    assert!(instance_is_rejected(&engine, &ordinary));

    drop(custom_stores.pop());
    let replacement_custom = live_instance(&engine, &custom)?;
    drop(ordinary_store);
    let replacement_ordinary = live_instance(&engine, &ordinary)?;

    drop((replacement_custom, replacement_ordinary, custom_stores));
    Ok(())
}

#[test]
fn page_size_1_pool_capacity_is_exact_product() -> Result<()> {
    let mut pool = small_pool();
    pool.total_component_instances(1)
        .total_memories(3)
        .page_size_1_memory_max_size(32)
        .max_page_size_1_memories_per_component(1);
    let engine = pooling_engine(pool)?;
    let custom = Module::new(&engine, "(module (memory 1 32 (pagesize 1)))")?;
    let ordinary = Module::new(&engine, "(module (memory 1 1))")?;

    let custom_store = live_instance(&engine, &custom)?;
    let mut ordinary_stores = Vec::new();
    for _ in 0..3 {
        ordinary_stores.push(live_instance(&engine, &ordinary)?);
    }

    assert!(instance_is_rejected(&engine, &custom));
    assert!(instance_is_rejected(&engine, &ordinary));

    drop((custom_store, ordinary_stores));
    Ok(())
}

#[test]
fn page_size_1_pool_size_and_growth_limits_are_independent() -> Result<()> {
    let mut pool = small_pool();
    pool.page_size_1_memory_max_size(7)
        .max_page_size_1_memories_per_component(1);
    let engine = pooling_engine(pool)?;
    let custom = Module::new(
        &engine,
        "(module (memory (export \"memory\") 1 (pagesize 1)))",
    )?;
    let ordinary = Module::new(&engine, "(module (memory 1 1))")?;

    let (mut custom_store, custom_instance) = instance_with_handle(&engine, &custom)?;
    let memory = custom_instance
        .get_memory(&mut custom_store, "memory")
        .unwrap();
    assert_eq!(memory.data_size(&custom_store), 1);
    assert_eq!(memory.grow(&mut custom_store, 6)?, 1);
    assert_eq!(memory.data_size(&custom_store), 7);
    assert!(memory.grow(&mut custom_store, 1).is_err());

    let ordinary_store = live_instance(&engine, &ordinary)?;
    assert!(module_is_rejected(
        &engine,
        "(module (memory 8 8 (pagesize 1)))"
    ));
    drop((ordinary_store, custom_store));

    let mut pool = small_pool();
    pool.page_size_1_memory_max_size(65_537)
        .max_page_size_1_memories_per_component(1);
    let engine = pooling_engine(pool)?;
    let larger_custom = Module::new(&engine, "(module (memory 65537 65537 (pagesize 1)))")?;
    live_instance(&engine, &larger_custom)?;
    assert!(module_is_rejected(&engine, "(module (memory 2 2))"));
    Ok(())
}

#[test]
fn page_size_1_pool_preserves_module_memory_limit() -> Result<()> {
    const MIXED_MODULE: &str = r#"
        (module
            (memory 1 1)
            (memory 1 32 (pagesize 1))
        )
    "#;

    let mut pool = small_pool();
    pool.max_memories_per_module(1)
        .page_size_1_memory_max_size(32)
        .max_page_size_1_memories_per_component(1);
    let engine = pooling_engine(pool)?;
    assert!(module_is_rejected(&engine, MIXED_MODULE));

    let mut pool = small_pool();
    pool.max_memories_per_module(2)
        .page_size_1_memory_max_size(32)
        .max_page_size_1_memories_per_component(1);
    let engine = pooling_engine(pool)?;
    let module = Module::new(&engine, MIXED_MODULE)?;
    live_instance(&engine, &module)?;
    Ok(())
}

#[test]
fn page_size_1_pool_component_limits_are_independent() -> Result<()> {
    let mut pool = small_pool();
    pool.total_component_instances(2)
        .total_core_instances(8)
        .max_core_instances_per_component(8)
        .total_memories(2)
        .max_memories_per_component(1)
        .page_size_1_memory_max_size(32)
        .max_page_size_1_memories_per_component(2);
    let engine = pooling_engine(pool)?;

    component_succeeds(
        &engine,
        r#"
            (component
                (core module $ordinary (memory 1 1))
                (core module $custom (memory 1 32 (pagesize 1)))
                (core instance $ordinary-instance (instantiate $ordinary))
                (core instance $custom-a (instantiate $custom))
                (core instance $custom-b (instantiate $custom))
            )
        "#,
    )?;

    assert!(component_is_rejected(
        &engine,
        r#"
            (component
                (core module $ordinary (memory 1 1))
                (core module $custom (memory 1 32 (pagesize 1)))
                (core instance $ordinary-a (instantiate $ordinary))
                (core instance $ordinary-b (instantiate $ordinary))
                (core instance $custom-instance (instantiate $custom))
            )
        "#,
    ));

    assert!(component_is_rejected(
        &engine,
        r#"
            (component
                (core module $ordinary (memory 1 1))
                (core module $custom (memory 1 32 (pagesize 1)))
                (core instance $ordinary-instance (instantiate $ordinary))
                (core instance $custom-a (instantiate $custom))
                (core instance $custom-b (instantiate $custom))
                (core instance $custom-c (instantiate $custom))
            )
        "#,
    ));
    Ok(())
}

#[test]
fn page_size_1_pool_fallback_preserves_component_limit() -> Result<()> {
    let mut pool = small_pool();
    pool.total_core_instances(4)
        .max_core_instances_per_component(4)
        .total_memories(3)
        .max_memories_per_component(2);
    let engine = pooling_engine(pool)?;

    assert!(component_is_rejected(
        &engine,
        r#"
            (component
                (core module $ordinary (memory 1 1))
                (core module $custom (memory 1 32 (pagesize 1)))
                (core instance $ordinary-instance (instantiate $ordinary))
                (core instance $custom-a (instantiate $custom))
                (core instance $custom-b (instantiate $custom))
            )
        "#,
    ));
    Ok(())
}

fn assert_custom_memory_is_reset(batch_size: usize) -> Result<()> {
    let mut pool = small_pool();
    pool.decommit_batch_size(batch_size)
        .page_size_1_memory_max_size(32)
        .max_page_size_1_memories_per_component(1);
    let engine = pooling_engine(pool)?;
    let module = Module::new(
        &engine,
        r#"
            (module
                (memory (export "memory") 4 32 (pagesize 1))
                (data (i32.const 0) "\01\02\03\04")
            )
        "#,
    )?;

    {
        let (mut store, instance) = instance_with_handle(&engine, &module)?;
        let memory = instance.get_memory(&mut store, "memory").unwrap();
        assert_eq!(&memory.data(&store)[..4], &[1, 2, 3, 4]);
        assert_eq!(memory.grow(&mut store, 12)?, 4);
        memory.data_mut(&mut store).fill(0xa5);
    }

    {
        let (mut store, instance) = instance_with_handle(&engine, &module)?;
        let memory = instance.get_memory(&mut store, "memory").unwrap();
        assert_eq!(memory.data_size(&store), 4);
        assert_eq!(&memory.data(&store)[..4], &[1, 2, 3, 4]);
        assert_eq!(memory.grow(&mut store, 12)?, 4);
        assert_eq!(&memory.data(&store)[..4], &[1, 2, 3, 4]);
        assert!(memory.data(&store)[4..16].iter().all(|byte| *byte == 0));
    }
    Ok(())
}

#[test]
fn page_size_1_pool_reuse_resets_memory() -> Result<()> {
    assert_custom_memory_is_reset(1)?;
    assert_custom_memory_is_reset(64)?;
    Ok(())
}

#[test]
fn page_size_1_pool_mixed_decommit_recycles_both_pools() -> Result<()> {
    let mut pool = small_pool();
    pool.decommit_batch_size(64)
        .page_size_1_memory_max_size(32)
        .max_page_size_1_memories_per_component(1);
    let engine = pooling_engine(pool)?;
    let custom = Module::new(
        &engine,
        r#"
            (module
                (memory (export "memory") 4 32 (pagesize 1))
                (data (i32.const 0) "tiny")
            )
        "#,
    )?;
    let ordinary = Module::new(
        &engine,
        r#"
            (module
                (memory (export "memory") 1 1)
                (data (i32.const 0) "ordinary")
            )
        "#,
    )?;

    {
        let (mut custom_store, custom_instance) = instance_with_handle(&engine, &custom)?;
        custom_instance
            .get_memory(&mut custom_store, "memory")
            .unwrap()
            .data_mut(&mut custom_store)[0] = 0xff;

        let (mut ordinary_store, ordinary_instance) = instance_with_handle(&engine, &ordinary)?;
        ordinary_instance
            .get_memory(&mut ordinary_store, "memory")
            .unwrap()
            .data_mut(&mut ordinary_store)[0] = 0xff;
    }

    let (mut ordinary_store, ordinary_instance) = instance_with_handle(&engine, &ordinary)?;
    let ordinary_memory = ordinary_instance
        .get_memory(&mut ordinary_store, "memory")
        .unwrap();
    assert_eq!(&ordinary_memory.data(&ordinary_store)[..8], b"ordinary");

    let (mut custom_store, custom_instance) = instance_with_handle(&engine, &custom)?;
    let custom_memory = custom_instance
        .get_memory(&mut custom_store, "memory")
        .unwrap();
    assert_eq!(&custom_memory.data(&custom_store)[..4], b"tiny");
    Ok(())
}

#[test]
fn page_size_1_pool_purges_dropped_modules_from_both_pools() -> Result<()> {
    let mut pool = small_pool();
    pool.max_unused_warm_slots(2)
        .decommit_batch_size(1)
        .page_size_1_memory_max_size(32)
        .max_page_size_1_memories_per_component(1);
    let engine = pooling_engine(pool)?;
    let custom = Module::new(
        &engine,
        r#"(module
            (memory (export "memory") 4 32 (pagesize 1))
            (data (i32.const 0) "tiny")
        )"#,
    )?;
    let ordinary = Module::new(
        &engine,
        r#"(module
            (memory (export "memory") 1 1)
            (data (i32.const 0) "ordinary")
        )"#,
    )?;

    let custom_store = live_instance(&engine, &custom)?;
    let ordinary_store = live_instance(&engine, &ordinary)?;
    drop((custom_store, ordinary_store));
    drop(custom);
    drop(ordinary);

    let replacement_custom = Module::new(
        &engine,
        r#"(module
            (memory (export "memory") 4 32 (pagesize 1))
            (data (i32.const 0) "new!")
        )"#,
    )?;
    let (mut custom_store, custom_instance) = instance_with_handle(&engine, &replacement_custom)?;
    let custom_memory = custom_instance
        .get_memory(&mut custom_store, "memory")
        .unwrap();
    assert_eq!(&custom_memory.data(&custom_store)[..4], b"new!");

    let replacement_ordinary = Module::new(
        &engine,
        r#"(module
            (memory (export "memory") 1 1)
            (data (i32.const 0) "replacement")
        )"#,
    )?;
    let (mut ordinary_store, ordinary_instance) =
        instance_with_handle(&engine, &replacement_ordinary)?;
    let ordinary_memory = ordinary_instance
        .get_memory(&mut ordinary_store, "memory")
        .unwrap();
    assert_eq!(&ordinary_memory.data(&ordinary_store)[..11], b"replacement");
    Ok(())
}

#[test]
fn page_size_1_pool_metrics_include_both_pools() -> Result<()> {
    let mut pool = small_pool();
    pool.decommit_batch_size(1)
        .page_size_1_memory_max_size(32)
        .max_page_size_1_memories_per_component(1);
    let engine = pooling_engine(pool)?;
    let metrics = engine.pooling_allocator_metrics().unwrap();
    let custom = Module::new(&engine, "(module (memory 1 32 (pagesize 1)))")?;
    let ordinary = Module::new(&engine, "(module (memory 1 1))")?;

    assert_eq!(metrics.memories(), 0);
    assert_eq!(metrics.unused_warm_memories(), 0);

    let custom_store = live_instance(&engine, &custom)?;
    let ordinary_store = live_instance(&engine, &ordinary)?;
    assert_eq!(metrics.memories(), 2);
    assert_eq!(metrics.unused_warm_memories(), 0);

    drop(ordinary_store);
    assert_eq!(metrics.memories(), 1);
    assert_eq!(metrics.unused_warm_memories(), 1);

    drop(custom_store);
    assert_eq!(metrics.memories(), 0);
    assert_eq!(metrics.unused_warm_memories(), 2);

    let custom_store = live_instance(&engine, &custom)?;
    assert_eq!(metrics.memories(), 1);
    assert_eq!(metrics.unused_warm_memories(), 1);
    drop(custom_store);
    Ok(())
}

fn custom_memory_type() -> Result<MemoryType> {
    MemoryTypeBuilder::new()
        .min(1)
        .max(Some(32))
        .page_size_log2(0)
        .build()
}

#[test]
fn page_size_1_pool_does_not_limit_host_created_memories() -> Result<()> {
    let mut pool = small_pool();
    pool.page_size_1_memory_max_size(32)
        .max_page_size_1_memories_per_component(1);
    let engine = pooling_engine(pool)?;
    let metrics = engine.pooling_allocator_metrics().unwrap();

    let mut custom_store = Store::new(&engine, ());
    let custom_memory = Memory::new(&mut custom_store, custom_memory_type()?)?;
    let mut larger_custom_store = Store::new(&engine, ());
    let larger_custom_memory = Memory::new(
        &mut larger_custom_store,
        MemoryTypeBuilder::new()
            .min(33)
            .max(Some(64))
            .page_size_log2(0)
            .build()?,
    )?;
    let mut ordinary_store = Store::new(&engine, ());
    let ordinary_memory = Memory::new(&mut ordinary_store, MemoryType::new(2, Some(2)))?;

    assert_eq!(custom_memory.data_size(&custom_store), 1);
    assert_eq!(larger_custom_memory.data_size(&larger_custom_store), 33);
    assert_eq!(ordinary_memory.data_size(&ordinary_store), 2 << 16);
    assert_eq!(metrics.memories(), 0);

    let custom_module = Module::new(&engine, "(module (memory 1 32 (pagesize 1)))")?;
    let ordinary_module = Module::new(&engine, "(module (memory 1 1))")?;
    let custom_module_store = live_instance(&engine, &custom_module)?;
    let ordinary_module_store = live_instance(&engine, &ordinary_module)?;

    assert_eq!(metrics.memories(), 2);
    assert!(instance_is_rejected(&engine, &custom_module));
    assert!(instance_is_rejected(&engine, &ordinary_module));

    drop((
        custom_module_store,
        ordinary_module_store,
        custom_memory,
        custom_store,
        larger_custom_memory,
        larger_custom_store,
        ordinary_memory,
        ordinary_store,
    ));
    Ok(())
}

fn assert_reflected_pool_options(options: &CommonOptions) {
    assert_eq!(options.opts.pooling_page_size_1_memory_max_size, Some(64));
    assert_eq!(
        options.opts.pooling_max_page_size_1_memories_per_component,
        Some(2)
    );
}

#[test]
fn page_size_1_pool_cli_options_round_trip() -> Result<()> {
    let mut options = CommonOptions::try_parse_from([
        "wasmtime",
        "-Opooling-allocator",
        "-Omemory-reservation=65536",
        "-Omemory-guard-size=0",
        "-Opooling-total-memories=1",
        "-Opooling-total-component-instances=1",
        "-Opooling-max-memory-size=65536",
        "-Opooling-page-size-1-memory-max-size=64",
        "-Opooling-max-page-size-1-memories-per-component=2",
        "-Wcustom-page-sizes",
    ])?;
    let config = options.config(None)?;
    let engine = Engine::new(&config)?;
    let reflected = CommonOptions::from_engine(&engine);
    assert_reflected_pool_options(&reflected);
    Ok(())
}

#[test]
fn page_size_1_pool_toml_options_round_trip() -> Result<()> {
    let mut options: CommonOptions = toml::from_str(
        r#"
            [optimize]
            pooling-allocator = true
            memory-reservation = 65536
            memory-guard-size = 0
            pooling-total-memories = 1
            pooling-total-component-instances = 1
            pooling-max-memory-size = 65536
            pooling-page-size-1-memory-max-size = 64
            pooling-max-page-size-1-memories-per-component = 2

            [wasm]
            custom-page-sizes = true
        "#,
    )?;
    let config = options.config(None)?;
    let engine = Engine::new(&config)?;
    let reflected = CommonOptions::from_engine(&engine);
    assert_reflected_pool_options(&reflected);
    Ok(())
}

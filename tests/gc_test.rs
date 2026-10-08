use indexmap::IndexMap;
use shae::gc::GcHeap;
use shae::value::Value;

#[test]
fn test_gc_basic_allocation_and_retrieval() {
    let mut heap = GcHeap::new();

    let arr_ref = heap.alloc_array(vec![Value::Int(10), Value::Int(20), Value::Int(30)]);
    let map_ref = heap.alloc_map({
        let mut m = IndexMap::new();
        m.insert("name".to_string(), Value::String("Shae".to_string()));
        m
    });
    let str_ref = heap.alloc_string("Hello Garbage Collector".to_string());

    assert_eq!(heap.live_objects_count(), 3);
    assert!(heap.bytes_allocated > 0);

    let arr = heap.as_array(arr_ref).unwrap();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[1], Value::Int(20));

    let map = heap.as_map(map_ref).unwrap();
    assert_eq!(map.get("name"), Some(&Value::String("Shae".to_string())));

    let s = heap.as_string(str_ref).unwrap();
    assert_eq!(s, "Hello Garbage Collector");
}

#[test]
fn test_gc_sweep_unreachable_objects() {
    let mut heap = GcHeap::new();

    let _unreachable1 = heap.alloc_array(vec![Value::Int(1), Value::Int(2)]);
    let _unreachable2 = heap.alloc_string("temporary data".to_string());

    assert_eq!(heap.live_objects_count(), 2);

    // Run GC with empty roots
    let stats = heap.collect_garbage(|_heap, _gray_stack| {
        // No roots marked!
    });

    assert_eq!(stats.objects_before, 2);
    assert_eq!(stats.freed_objects, 2);
    assert_eq!(stats.objects_after, 0);
    assert_eq!(heap.live_objects_count(), 0);
    assert_eq!(heap.bytes_allocated, 0);
    assert_eq!(heap.free_list.len(), 2);
}

#[test]
fn test_gc_preserves_reachable_roots_and_nested_references() {
    let mut heap = GcHeap::new();

    // Leaf array
    let leaf_ref = heap.alloc_array(vec![Value::Int(42), Value::Int(99)]);

    // Root map containing the leaf array as a GcArray value
    let root_map_ref = heap.alloc_map({
        let mut m = IndexMap::new();
        m.insert("items".to_string(), Value::GcArray(leaf_ref));
        m
    });

    // Unreachable junk object
    let _junk = heap.alloc_string("junk that should be collected".to_string());

    assert_eq!(heap.live_objects_count(), 3);

    // Run GC marking only root_map_ref
    let stats = heap.collect_garbage(|h, gray| {
        h.mark_ref(root_map_ref, gray);
    });

    // Junk was freed, root_map and leaf array were preserved!
    assert_eq!(stats.freed_objects, 1);
    assert_eq!(stats.objects_after, 2);
    assert_eq!(heap.live_objects_count(), 2);

    // Verify root_map and leaf still contain intact data
    let root_map = heap.as_map(root_map_ref).unwrap();
    if let Some(Value::GcArray(inner_ref)) = root_map.get("items") {
        assert_eq!(*inner_ref, leaf_ref);
        let inner_arr = heap.as_array(*inner_ref).unwrap();
        assert_eq!(inner_arr, &vec![Value::Int(42), Value::Int(99)]);
    } else {
        panic!("Expected GcArray in root_map");
    }
}

#[test]
fn test_gc_collects_cyclic_references() {
    let mut heap = GcHeap::new();

    // Create Array A and Array B
    let a_ref = heap.alloc_array(vec![Value::Int(1)]);
    let b_ref = heap.alloc_array(vec![Value::Int(2)]);

    // Make them point to each other: A -> B, B -> A
    heap.as_array_mut(a_ref).unwrap().push(Value::GcArray(b_ref));
    heap.as_array_mut(b_ref).unwrap().push(Value::GcArray(a_ref));

    assert_eq!(heap.live_objects_count(), 2);

    // With no external roots, this cycle should be completely collected
    let stats = heap.collect_garbage(|_h, _gray| {});

    assert_eq!(stats.freed_objects, 2);
    assert_eq!(stats.objects_after, 0);
    assert_eq!(heap.live_objects_count(), 0);
    assert_eq!(heap.bytes_allocated, 0);
}

#[test]
fn test_gc_slot_reuse_after_sweep() {
    let mut heap = GcHeap::new();

    let r1 = heap.alloc_string("first".to_string());
    let r2 = heap.alloc_string("second".to_string());

    assert_eq!(r1.0, 0);
    assert_eq!(r2.0, 1);

    // Collect all
    heap.collect_garbage(|_h, _gray| {});
    assert_eq!(heap.live_objects_count(), 0);

    // Allocate again - should reuse freed indices in reverse order
    let r3 = heap.alloc_string("third".to_string());
    let r4 = heap.alloc_string("fourth".to_string());

    assert_eq!(r3.0, 1);
    assert_eq!(r4.0, 0);
    assert_eq!(heap.live_objects_count(), 2);
}

#[test]
fn test_gc_mutation_without_locks() {
    let mut heap = GcHeap::new();

    let arr_ref = heap.alloc_array(vec![Value::Int(10), Value::Int(20)]);

    // Direct mutable access without RwLock overhead
    {
        let arr = heap.as_array_mut(arr_ref).unwrap();
        arr.push(Value::Int(30));
        arr[0] = Value::Int(999);
    }

    let arr = heap.as_array(arr_ref).unwrap();
    assert_eq!(arr, &vec![Value::Int(999), Value::Int(20), Value::Int(30)]);
}

#[test]
fn test_gc_stress_mode_and_threshold_triggers() {
    let mut heap = GcHeap::with_threshold(100);
    heap.stress_gc = true;

    assert!(heap.should_collect());

    // Allocate 100 small arrays in stress mode
    let mut kept = Vec::new();
    for i in 0..100 {
        let r = heap.alloc_array(vec![Value::Int(i)]);
        if i % 10 == 0 {
            kept.push(r);
        }
        // Run GC every 5 iterations
        if i % 5 == 0 {
            heap.collect_garbage(|h, gray| {
                for k in &kept {
                    h.mark_ref(*k, gray);
                }
            });
        }
    }

    // Final GC
    heap.collect_garbage(|h, gray| {
        for k in &kept {
            h.mark_ref(*k, gray);
        }
    });

    assert_eq!(heap.live_objects_count(), kept.len());
    for (idx, r) in kept.iter().enumerate() {
        let arr = heap.as_array(*r).unwrap();
        assert_eq!(arr[0], Value::Int((idx * 10) as i64));
    }
}

#[test]
fn test_vm_gc_builtin_execution() {
    let script = r#"
gc()
"#;
    let program = shae::parse_source(script).unwrap();
    let chunk = shae::compiler::Compiler::new().compile_program(&program).unwrap();
    let mut vm = shae::vm::VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, shae::vm::InterpretResult::Ok(Value::Int(0)));
}

#[test]
fn test_vm_gc_collects_unreachable_heap_values() {
    let mut vm = shae::vm::VM::new();

    // Allocate 5 temporary heap arrays
    for i in 0..5 {
        vm.heap.alloc_array(vec![Value::Int(i)]);
    }
    assert_eq!(vm.heap.live_objects_count(), 5);

    // Call gc() via VM
    let stats = vm.collect_garbage();
    assert_eq!(stats.freed_objects, 5);
    assert_eq!(vm.heap.live_objects_count(), 0);
}

#[test]
fn test_vm_gc_preserves_globals_and_stack() {
    let mut vm = shae::vm::VM::new();

    let root_ref = vm.heap.alloc_array(vec![Value::Int(100), Value::Int(200)]);
    vm.globals.insert("my_global".to_string(), Value::GcArray(root_ref));

    let _garbage = vm.heap.alloc_string("temporary string".to_string());
    assert_eq!(vm.heap.live_objects_count(), 2);

    let stats = vm.collect_garbage();
    assert_eq!(stats.freed_objects, 1);
    assert_eq!(vm.heap.live_objects_count(), 1);

    // Verify my_global survived and has correct data
    if let Some(Value::GcArray(r)) = vm.globals.get("my_global") {
        assert_eq!(*r, root_ref);
        let arr = vm.heap.as_array(*r).unwrap();
        assert_eq!(arr, &vec![Value::Int(100), Value::Int(200)]);
    } else {
        panic!("my_global not found in globals");
    }
}


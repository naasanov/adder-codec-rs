#!/bin/bash

echo "File Size Comparison"
echo "===================="
echo ""

for file in bunny lake; do
    echo "=== ${file^^} ==="
    
    raw_size=$(stat -f%z ~/projects/raw/${file}.adder 2>/dev/null || stat -c%s ~/projects/raw/${file}.adder 2>/dev/null)
    main_size=$(stat -f%z ~/projects/main/${file}.adder 2>/dev/null || stat -c%s ~/projects/main/${file}.adder 2>/dev/null)
    rle_size=$(stat -f%z ~/projects/rle/${file}.adder 2>/dev/null || stat -c%s ~/projects/rle/${file}.adder 2>/dev/null)
    rle_context_size=$(stat -f%z ~/projects/rle+context/${file}.adder 2>/dev/null || stat -c%s ~/projects/rle+context/${file}.adder 2>/dev/null)
    
    echo "RAW:         $(numfmt --to=iec-i --suffix=B $raw_size) ($raw_size bytes)"
    echo "MAIN:        $(numfmt --to=iec-i --suffix=B $main_size) ($main_size bytes)"
    echo "RLE:         $(numfmt --to=iec-i --suffix=B $rle_size) ($rle_size bytes)"
    echo "RLE+CONTEXT: $(numfmt --to=iec-i --suffix=B $rle_context_size) ($rle_context_size bytes)"
    echo ""
    
    main_pct=$(awk "BEGIN {printf \"%.2f\", ($main_size/$raw_size)*100}")
    rle_pct=$(awk "BEGIN {printf \"%.2f\", ($rle_size/$raw_size)*100}")
    rle_context_pct=$(awk "BEGIN {printf \"%.2f\", ($rle_context_size/$raw_size)*100}")
    
    main_vs_raw=$(awk "BEGIN {printf \"%.2f\", (($raw_size-$main_size)/$raw_size)*100}")
    rle_vs_raw=$(awk "BEGIN {printf \"%.2f\", (($raw_size-$rle_size)/$raw_size)*100}")
    rle_context_vs_raw=$(awk "BEGIN {printf \"%.2f\", (($raw_size-$rle_context_size)/$raw_size)*100}")
    
    rle_vs_main=$(awk "BEGIN {printf \"%.2f\", (($main_size-$rle_size)/$main_size)*100}")
    rle_context_vs_main=$(awk "BEGIN {printf \"%.2f\", (($main_size-$rle_context_size)/$main_size)*100}")
    
    echo "Compression ratios (vs RAW):"
    echo "  MAIN:        ${main_pct}% of raw (${main_vs_raw}% reduction)"
    echo "  RLE:         ${rle_pct}% of raw (${rle_vs_raw}% reduction)"
    echo "  RLE+CONTEXT: ${rle_context_pct}% of raw (${rle_context_vs_raw}% reduction)"
    echo ""
    echo "Improvement over MAIN:"
    echo "  RLE:         ${rle_vs_main}%"
    echo "  RLE+CONTEXT: ${rle_context_vs_main}%"
    echo ""
    echo "----------------------------------------"
    echo ""
done

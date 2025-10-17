#!/usr/bin/env bash

routes=("/api/users" "/api/posts" "/api/comments" "/api/auth" "/api/products")

while true; do
    route=${routes[$RANDOM % ${#routes[@]}]}
    jittery=$((RANDOM % 100))
    latency=$((100 + jittery))
    
    echo "{\"route\":\"$route\",\"latency\":$latency}"
    
    if (( RANDOM % 10 == 0 )); then
        echo "ERROR: Database connection failed for $route"
    fi
    
    if (( RANDOM % 15 == 0 )); then
        echo "WARN: Slow query detected on $route"
    fi
    
done

#!/usr/bin/env bash

routes=("/api/users" "/api/posts" "/api/comments" "/api/auth" "/api/products")

while true; do
    route=${routes[$RANDOM % ${#routes[@]}]}
    latency=$((50 + RANDOM % 500))
    
    echo "{\"route\":\"$route\",\"latency\":$latency}"
    
    if (( RANDOM % 10 == 0 )); then
        echo "ERROR: Database connection failed for $route"
    fi
    
    if (( RANDOM % 15 == 0 )); then
        echo "WARN: Slow query detected on $route"
    fi
    
    sleep 0.1
done

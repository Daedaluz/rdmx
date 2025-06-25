#!/bin/bash
docker buildx build --platform arm64,amd64 -t ghcr.io/daedaluz/rdmx --push .

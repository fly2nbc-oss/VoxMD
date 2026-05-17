#!/bin/bash
set -eu

SDK="${HOME}/.local/vulkan-sdk"

if [ -f /usr/include/vulkan/vulkan.h ]; then
  echo "Vulkan headers: system (/usr/include/vulkan)"
  export VULKAN_SDK=/usr
  exit 0
fi

if [ ! -f "${SDK}/include/vulkan/vulkan.h" ]; then
  REPO="${HOME}/.local/share/vulkan-headers"
  echo "Vulkan headers: cloning Khronos Vulkan-Headers into ${REPO} ..."
  mkdir -p "$(dirname "${REPO}")"
  if [ ! -d "${REPO}/.git" ]; then
    git clone --depth 1 https://github.com/KhronosGroup/Vulkan-Headers.git "${REPO}"
  else
    git -C "${REPO}" pull --ff-only
  fi
  mkdir -p "${SDK}"
  ln -sfn "${REPO}/include" "${SDK}/include"
fi

export VULKAN_SDK="${SDK}"
echo "VULKAN_SDK=${VULKAN_SDK}"

#!/bin/sh
# @Author: BlahGeek
# @Date:   2017-06-18
# @Last Modified by:   BlahGeek
# @Last Modified time: 2017-06-18

RES="$(echo "$1" | bc -l)"
echo "{\"results\": [{\"title\": \"${RES}\", \"subtitle\": \"= $1\"}]}"

#/bin/sh

#nodes="1:127.0.0.1:11111;2:127.0.0.1:11112;3:127.0.0.1:11113;4:127.0.0.1:11114;5:127.0.0.1:11115"
nodes="1:127.0.0.1:11111;2:127.0.0.1:11112;3:127.0.0.1:11113;4:127.0.0.1:11114"
binary=./target/debug/vaba-server
echo "$nodes"

./stop.sh

for i in {1..4}; do
 echo "start node $i"
 $binary --id $i -n $nodes &
done

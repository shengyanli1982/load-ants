import dns.message
import dns.rdatatype

# 创建DNS查询消息
query = dns.message.make_query("example.com", dns.rdatatype.A)

# 将DNS消息转换为二进制格式
wire_message = query.to_wire()

# 保存到文件
with open("dns-message.bin", "wb") as f:
    f.write(wire_message)

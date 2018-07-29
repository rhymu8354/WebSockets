#ifndef WEB_SOCKETS_WEB_SOCKET_HPP
#define WEB_SOCKETS_WEB_SOCKET_HPP

/**
 * @file WebSocket.hpp
 *
 * This module declares the WebSockets::WebSocket class.
 *
 * © 2018 by Richard Walters
 */

#include <functional>
#include <Http/Connection.hpp>
#include <memory>
#include <string>

namespace WebSockets {

    /**
     * This class implements the WebSocket protocol, in either client
     * or server role.  The WebSocket protocol is documented in
     * [RFC 6455](https://tools.ietf.org/html/rfc6455).
     */
    class WebSocket {
        // Types
    public:
        /**
         * This identifies which role the local endpoint originally
         * had when the WebSocket was established.
         */
        enum class Role {
            /**
             * In this role, the data for messages sent must be masked,
             * and all messages received must not be masked.
             */
            Client,

            /**
             * In this role, the data for messages sent must not be masked,
             * and all messages received must be masked.
             */
            Server,
        };

        /**
         * This is the type of function used to publish messages received
         * by the WebSocket.
         *
         * @param[in] data
         *     This is the payload data from the received message.
         */
        typedef std::function< void(const std::string& data) > MessageReceivedDelegate;

        // Lifecycle management
    public:
        ~WebSocket();
        WebSocket(const WebSocket&) = delete;
        WebSocket(WebSocket&&) = delete;
        WebSocket& operator=(const WebSocket&) = delete;
        WebSocket& operator=(WebSocket&&) = delete;

        // Public methods
    public:
        /**
         * This is the default constructor.
         */
        WebSocket();

        /**
         * This method puts the WebSocket into the OPENED state,
         * in the given role.
         *
         * @param[in] connection
         *     This is the connection to use to send and receive frames.
         *
         * @param[in] role
         *     This is the role to play in the connection.
         */
        void Open(
            std::shared_ptr< Http::Connection > connection,
            Role role
        );

        /**
         * This method sends a ping message over the WebSocket.
         *
         * @param[in] data
         *     This is the optional data to include with the message.
         */
        void Ping(const std::string& data = "");

        /**
         * This method sends an unsolicited pong message over the WebSocket.
         *
         * @param[in] data
         *     This is the optional data to include with the message.
         */
        void Pong(const std::string& data = "");

        /**
         * This method sends a text message, or fragment thereof,
         * over the WebSocket.
         *
         * @param[in] data
         *     This is the data to include with the message.
         *
         * @param[in] lastFragment
         *     This indicates whether or not this is the last
         *     frame in its message.
         */
        void SendText(
            const std::string& data,
            bool lastFragment = true
        );

        /**
         * This method sends a binary message, or fragment thereof,
         * over the WebSocket.
         *
         * @param[in] data
         *     This is the data to include with the message.
         *
         * @param[in] lastFragment
         *     This indicates whether or not this is the last
         *     frame in its message.
         */
        void SendBinary(
            const std::string& data,
            bool lastFragment = true
        );

        /**
         * This method sets the function to call whenever a ping message
         * is received by the WebSocket.
         *
         * @param[in] pingDelegate
         *     This is the function to call whenever a ping message
         *     is received by the WebSocket.
         */
        void SetPingDelegate(MessageReceivedDelegate pingDelegate);

        /**
         * This method sets the function to call whenever a pong message
         * is received by the WebSocket.
         *
         * @param[in] pongDelegate
         *     This is the function to call whenever a pong message
         *     is received by the WebSocket.
         */
        void SetPongDelegate(MessageReceivedDelegate pongDelegate);

        /**
         * This method sets the function to call whenever a text message
         * is received by the WebSocket.
         *
         * @param[in] textDelegate
         *     This is the function to call whenever a text message
         *     is received by the WebSocket.
         */
        void SetTextDelegate(MessageReceivedDelegate textDelegate);

        /**
         * This method sets the function to call whenever a binary message
         * is received by the WebSocket.
         *
         * @param[in] binaryDelegate
         *     This is the function to call whenever a binary message
         *     is received by the WebSocket.
         */
        void SetBinaryDelegate(MessageReceivedDelegate binaryDelegate);

        // Private properties
    private:
        /**
         * This is the type of structure that contains the private
         * properties of the instance.  It is defined in the implementation
         * and declared here to ensure that it is scoped inside the class.
         */
        struct Impl;

        /**
         * This contains the private properties of the instance.
         */
        std::unique_ptr< struct Impl > impl_;
    };

}

#endif /* WEB_SOCKETS_WEB_SOCKET_HPP */

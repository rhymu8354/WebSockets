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
#include <Http/Request.hpp>
#include <Http/Response.hpp>
#include <memory>
#include <string>
#include <SystemAbstractions/DiagnosticsSender.hpp>

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

        /**
         * This is the type of function used to notify the user that
         * the WebSocket has received a close frame or has been
         * closed due to an error.
         *
         * @param[in] code
         *     This is the status code from the received close frame.
         *
         * @param[in] reason
         *     This is the payload data from the received close frame.
         */
        typedef std::function<
            void(
                unsigned int code,
                const std::string& reason
            )
        > CloseReceivedDelegate;

        // Lifecycle management
    public:
        ~WebSocket() noexcept;
        WebSocket(const WebSocket&) = delete;
        WebSocket(WebSocket&&) noexcept;
        WebSocket& operator=(const WebSocket&) = delete;
        WebSocket& operator=(WebSocket&&) noexcept;

        // Public methods
    public:
        /**
         * This is the default constructor.
         */
        WebSocket();

        /**
         * This method forms a new subscription to diagnostic
         * messages published by the WebSocket.
         *
         * @param[in] delegate
         *     This is the function to call to deliver messages
         *     to the subscriber.
         *
         * @param[in] minLevel
         *     This is the minimum level of message that this subscriber
         *     desires to receive.
         *
         * @return
         *     A function is returned which may be called
         *     to terminate the subscription.
         */
        SystemAbstractions::DiagnosticsSender::UnsubscribeDelegate SubscribeToDiagnostics(
            SystemAbstractions::DiagnosticsSender::DiagnosticMessageDelegate delegate,
            size_t minLevel = 0
        );

        /**
         * This method puts the WebSocket into the OPENING state,
         * in the client role, updating the given HTTP request
         * in order to perform the opening handshake.
         *
         * @param[in,out] request
         *     This is the HTTP request that will be used to perform
         *     the opening handshake of the WebSocket.  This method
         *     updates the request to include the proper headers.
         */
        void StartOpenAsClient(
            Http::Request& request
        );

        /**
         * This method puts the WebSocket into the OPENED state,
         * in the client role, by checking the given HTTP response
         * returned that completes the handshake.
         *
         * @param[in] connection
         *     This is the connection to use to send and receive frames.
         *
         * @param[in] response
         *     This is the HTTP response that completed the opening
         *     handshake of the WebSocket.  This response may indicate
         *     that the handshake failed.
         *
         * @return
         *     An indication of whether or not the opening handshake
         *     succeeded is returned.
         */
        bool FinishOpenAsClient(
            std::shared_ptr< Http::Connection > connection,
            const Http::Response& response
        );

        /**
         * This method puts the WebSocket into the OPENED state,
         * in the server role, by checking the given HTTP request
         * and updating the given HTTP response accordingly,
         * to perform the opening handshake.
         *
         * @param[in] connection
         *     This is the connection to use to send and receive frames.
         *
         * @param[in] request
         *     This is the HTTP request that initiated the opening
         *     handshake of the WebSocket.
         *
         * @param[in,out] response
         *     This is the HTTP response that completes the opening
         *     handshake of the WebSocket.  This response may be updated
         *     to indicate that the handshake failed.
         *
         * @param[in] trailer
         *     This holds any characters that have already been received
         *     from the connection but come after the end of the open
         *     request.
         *
         * @return
         *     An indication of whether or not the opening handshake
         *     succeeded is returned.
         */
        bool OpenAsServer(
            std::shared_ptr< Http::Connection > connection,
            const Http::Request& request,
            Http::Response& response,
            const std::string& trailer
        );

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
         * This method initiates the closing of the WebSocket,
         * sending a close frame with the given status code and reason.
         *
         * @param[in] code
         *     This is the status code to send in the close frame.
         *
         * @param[in] reason
         *     This is the reason text to send in the close frame.
         */
        void Close(
            unsigned int code = 1005,
            const std::string reason = ""
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
         * This method sets the function to call whenever the WebSocket
         * has received a close frame or has been closed due to an error.
         *
         * @param[in] closeDelegate
         *     This is the function to call whenever the WebSocket
         *     has received a close frame or has been closed due to an error.
         */
        void SetCloseDelegate(CloseReceivedDelegate closeDelegate);

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
        std::shared_ptr< Impl > impl_;
    };

}

#endif /* WEB_SOCKETS_WEB_SOCKET_HPP */

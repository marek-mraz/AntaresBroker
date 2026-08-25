*** Settings ***
Documentation       Check that the Mqtt Notification is received with different userInfo and port

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Resource            ${EXECDIR}/resources/mqttUtils/MqttUtils.resource

Test Teardown       After Test
Test Template       Receive MQTT Notification


*** Variables ***
${subscription_payload_file_path}       subscriptions/subscription-building-entities-default.jsonld
${entity_building_filepath}             building-simple-attributes.jsonld
${fragment_filename}                    airQualityLevel-fragment.jsonld
${topic}                                ngsild-test-suite/topic


*** Test Cases ***
058_01_01 Without User And Port
    [Tags]    sub-mqtt-notification    5_8_6
    mosquitto.conf    mqtt://${mqtt_broker_host}/${topic}
058_01_02 With Non Default Port
    [Tags]    sub-mqtt-notification    5_8_6
    mosquitto.conf    mqtt://${mqtt_broker_host}:${mqtt_broker_non_default_port}/${topic}    port=${mqtt_broker_non_default_port}
058_01_03 With User
    [Tags]    sub-mqtt-notification    5_8_6
    mosquitto.conf    mqtt://user@${mqtt_broker_host}/${topic}
058_01_04 With User And Password
    [Tags]    sub-mqtt-notification    5_8_6
    mosquitto_with_user.conf    mqtt://user_with_password:password@${mqtt_broker_host}/${topic}    username=user_with_password    password=password
058_01_05 With User Password And Non Default Port
    [Tags]    sub-mqtt-notification    5_8_6
    mosquitto_with_user.conf    mqtt://user_with_password:password@${mqtt_broker_host}:${mqtt_broker_non_default_port}/${topic}    username=user_with_password    password=password    port=${mqtt_broker_non_default_port}


*** Keywords ***
Receive MQTT Notification
    [Documentation]    Check that the broker uses the authentication information from the endpoint url
    [Arguments]    ${config_filename}    ${endpoint_uri}    ${port}=1883    ${username}=${None}    ${password}=${None}

    Start Mqtt Server    ${config_filename}    ${port}
    Setup Mqtt Subscription    ${endpoint_uri}

    Set Username And Password    ${username}    ${password}
    Connect    ${mqtt_broker_host}    ${port}
    Subscribe    topic=${topic}    qos=1

    ${response}=    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}
    ${listenMessages}=    Listen    ${topic}
    Check Messages Contain One Instance    ${listenMessages}

After Test
    Delete Initial Subscriptions
    Delete Initial Entity
    Stop Mqtt Server
    Disconnect

Setup Mqtt Subscription
    [Arguments]    ${endpoint_uri}
    ${subscription_id}=    Generate Random Subscription Id
    ${entity_id}=    Generate Random Building Entity Id

    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ...    ${endpoint_uri}
    ${subscription_payload}=    Set Entity Id In Subscription    ${subscription_payload}    ${entity_id}

    Set Suite Variable    ${subscription_id}
    Set Suite Variable    ${entity_id}

    ${response}=    Create Entity    ${entity_building_filepath}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}
    Sleep    1s
    ${response}=    Create Subscription From Subscription Payload    ${subscription_payload}    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    Sleep    1s

Delete Initial Subscriptions
    Delete Subscription    ${subscription_id}

Delete Initial Entity
    Delete Entity    ${entity_id}

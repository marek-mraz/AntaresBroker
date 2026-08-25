*** Settings ***
Documentation       Common keywords used in setup steps for subscriptions

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Keywords ***
Create Subscription And Entity
    [Tags]    actor_context-subscriber    actor_context-producer
    [Arguments]    ${subscription_payload_file_path}    ${building_filename}    ${entity_id_suffix}=None

    IF    '${entity_id_suffix}' == None
        ${entity_id}=    Generate Random Building Entity Id
    ELSE
        ${entity_id}=    Catenate    ${BUILDING_ID_PREFIX}${entity_id_suffix}
    END
    Set Suite Variable    ${entity_id}
    ${create_entity_response}=    Create Entity Selecting Content Type
    ...    ${building_filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_entity_response.status_code}

    ${subscription_id}=    Generate Random Subscription Id
    Set Suite Variable    ${subscription_id}
    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ${create_subscription_response}=    Create Subscription From Subscription Payload
    ...    ${subscription_payload}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_subscription_response.status_code}

Create Subscription And Entity With Linked Entity
    [Tags]    actor_context-subscriber    actor_context-producer
    [Arguments]
    ...    ${subscription_payload_file_path}
    ...    ${linking_entity_filename}
    ...    ${linking_entity_id_suffix}
    ...    ${linked_entity_filename}
    ...    ${linked_entity_id_suffix}

    ${linking_entity_id}=    Catenate    ${BUILDING_ID_PREFIX}${linking_entity_id_suffix}
    Set Suite Variable    ${linking_entity_id}
    ${create_entity_response}=    Create Entity Selecting Content Type
    ...    ${linking_entity_filename}
    ...    ${linking_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_entity_response.status_code}

    ${linked_entity_id}=    Catenate    ${CITY_ID_PREFIX}${linked_entity_id_suffix}
    Set Suite Variable    ${linked_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${linked_entity_filename}
    ...    ${linked_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

    ${subscription_id}=    Generate Random Subscription Id
    Set Suite Variable    ${subscription_id}
    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ${create_subscription_response}=    Create Subscription From Subscription Payload
    ...    ${subscription_payload}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_subscription_response.status_code}

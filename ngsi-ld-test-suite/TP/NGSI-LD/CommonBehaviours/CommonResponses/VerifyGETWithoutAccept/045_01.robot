*** Settings ***
Documentation       Verify that on a GET HTTP request if nothing is specified on the Accept header, "application/json" is assumed

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${building_filename}=           building-simple-attributes.jsonld
${subscription_filename}=       subscriptions/subscription.jsonld
${registration_filename}=       csourceRegistrations/context-source-registration-with-expiration.jsonld
${registration_type}=           Vehicle
${tea_filename}=                vehicle-temporal-representation.jsonld
${teatype}=                     Vehicle
${content_type}=                application/json


*** Test Cases ***
045_01_01 Endpoint /entities/{entityId}
    [Documentation]    Verify that on a GET HTTP request if nothing is specified on the Accept header, "application/json" is assumed (/entities/{entityId})
    [Tags]    e-query    cb-get    6_3_4
    ${id}=    Generate Random Building Entity Id
    ${response}=    Create Entity Selecting Content Type
    ...    ${building_filename}
    ...    ${id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity
    ...    id=${id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=*/*
    Check Response Status Code    200    ${response.status_code}
    Check Response Headers Containing Content-Type set to    ${content_type}    ${response.headers}
    Check Response Headers Link Not Empty    ${response.headers}
    [Teardown]    Delete Entity    ${id}

045_01_02 Endpoint /subscriptions/{subscriptionId}
    [Documentation]    Verify that on a GET HTTP request if nothing is specified on the Accept header, "application/json" is assumed (/subscriptions/{subscriptionId})
    [Tags]    sub-retrieve    cb-get    6_3_4
    ${id}=    Generate Random Subscription Id
    ${response}=    Create Subscription    ${id}    ${subscription_filename}    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Subscription
    ...    id=${id}
    ...    accept=*/*
    Check Response Status Code    200    ${response.status_code}
    Check Response Headers Containing Content-Type set to    ${content_type}    ${response.headers}
    Check Response Headers Link Not Empty    ${response.headers}
    [Teardown]    Delete Subscription    ${id}

045_01_03 Endpoint /csourceRegistrations/
    [Documentation]    Verify that on a GET HTTP request if nothing is specified on the Accept header, "application/json" is assumed (/csourceRegistrations/)
    [Tags]    csr-query    cb-get    6_3_4
    ${registration_id}=    Generate Random CSR Id
    ${payload}=    Load JSON From File    ${EXECDIR}/data/${registration_filename}
    ${updated_payload}=    Update Value To JSON    ${payload}    $..id    ${registration_id}
    ${response}=    Create Context Source Registration With Return    ${updated_payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Query Context Source Registrations With Return
    ...    id=${registration_id}
    ...    type=${registration_type}
    ...    context=${ngsild_test_suite_context}
    ...    accept=*/*
    Check Response Status Code    200    ${response.status_code}
    Check Response Headers Containing Content-Type set to    ${content_type}    ${response.headers}
    Check Response Headers Link Not Empty    ${response.headers}
    [Teardown]    Delete Context Source Registration    ${registration_id}

045_01_04 Endpoint /temporal/entities
    [Documentation]    Verify that on a GET HTTP request if nothing is specified on the Accept header, "application/json" is assumed (/temporal/entities)
    [Tags]    te-query    cb-get    6_3_4
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${response}=    Create Or Update Temporal Representation Of Entity Selecting Content Type
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    filename=${tea_filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Query Temporal Representation Of Entities With Return
    ...    entity_types=${teatype}
    ...    timerel=after
    ...    timeAt=2020-08-01T12:05:00Z
    ...    context=${ngsild_test_suite_context}
    ...    accept=*/*
    Check Response Status Code    200    ${response.status_code}
    Set Test Variable    ${response}
    Check Response Headers Containing Content-Type set to    ${content_type}    ${response.headers}
    Check Response Headers Link Not Empty    ${response.headers}
    [Teardown]    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}

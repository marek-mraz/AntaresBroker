*** Settings ***
Documentation       Verify that with redirect registrations, query requests are correctly forwarded to Context Source

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Teardown       Delete Registration And Stop Context Source Mock Server
Test Template       Setup Registration And Context Source Mock Server And Query The Context Broker With Type


*** Variables ***
${entity_id_prefix}                     urn:ngsi-ld:Vehicle:
${entity_payload_filename}              vehicle-simple-attributes.json
${registration_id_prefix}               urn:ngsi-ld:Registration:
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-complete.jsonld


*** Test Cases ***
D011_02_red_01 Query The Context Broker With Type With queryEntity
    [Documentation]    Verify that when querying by entity type with a redirect registration that supports queryEntity,
    ...    the request is forwarded to the Context Source and entities are retrieved correctly
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-redirect    4_3_6_3    5_7_2
    GET    /broker1/ngsi-ld/v1/entities?type=Vehicle
D011_02_red_02 Query The Context Broker With Type With queryBatch
    [Documentation]    Verify that when querying by entity type with a redirect registration that supports queryBatch,
    ...    the request is forwarded to the Context Source and entities are retrieved correctly
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-redirect    4_3_6_3    5_7_2
    POST    /broker1/ngsi-ld/v1/entityOperations/query


*** Keywords ***
Setup Registration And Context Source Mock Server And Query The Context Broker With Type
    [Documentation]    Verify that when querying by entity type with a redirect registration that supports the requested
    ...    operation, the request is forwarded to the Context Source and entities are retrieved correctly
    [Arguments]    ${method}    ${url}
    ${entity_id}=    Generate Random Vehicle Entity Id

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    IF    '${method}' == 'POST'
        ${operations}=    Create List    queryBatch
    ELSE
        ${operations}=    Create List    queryEntity
    END
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    mode=redirect
    ...    endpoint=/broker1
    ...    operations=${operations}
    ${response1}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response1.status_code}

    Start Context Source Mock Server

    ${serialized_entity}=    Load Entity As Serialized Array    ${entity_payload_filename}    ${entity_id}
    Set Stub Reply    ${method}    ${url}    200    ${serialized_entity}

    IF    '${method}' == 'GET'
        ${response}=    Query Entities    entity_types=Vehicle    context=${ngsild_test_suite_context}
    ELSE IF    '${method}' == 'POST'
        ${entity_selector}=    Create Dictionary    type=Vehicle
        @{entities}=    Create List    ${entity_selector}
        ${response}=    Query Entities Via POST
        ...    entities=${entities}
        ...    content_type=${CONTENT_TYPE_LD_JSON}
        ...    accept=${CONTENT_TYPE_JSON}
        ...    context=${ngsild_test_suite_context}
    END

    Check Response Status Code    200    ${response.status_code}
    @{entities_id}=    Create List    ${entity_id}
    Check Response Body Containing Entities URIS set to    ${entities_id}    ${response.json()}

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server

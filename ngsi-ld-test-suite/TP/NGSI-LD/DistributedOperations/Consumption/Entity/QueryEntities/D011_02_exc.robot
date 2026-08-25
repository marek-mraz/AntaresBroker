*** Settings ***
Documentation       Verify that an entity can be retrieved by Id with a queryEntity and a queryBatch while using an
...                 exclusive registration.

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
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${entity_speed_filename}                vehicle-speed-attribute.json
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld


*** Test Cases ***
D011_02_exc_01 Query The Context Broker With Type With queryEntity
    [Documentation]    Check that if one queries the Context Broker for type, and the registration only allows the
    ...    queryEntity operation, the request is forwarded as expected
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-exclusive    4_3_6_3    5_7_2
    GET    /broker1/ngsi-ld/v1/entities
D011_02_exc_02 Query The Context Broker With Type With queryBatch
    [Documentation]    Check that if one queries the Context Broker for type, and the registration only allows the
    ...    queryBatch operation, the request is forwarded as expected
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-exclusive    4_3_6_3    5_7_2
    POST    /broker1/ngsi-ld/v1/entityOperations/query


*** Keywords ***
Setup Registration And Context Source Mock Server And Query The Context Broker With Type
    [Documentation]    Check that if one queries the Context Broker for type, and the registration only allows the
    ...    requested operation, the request is forwarded as expected
    [Arguments]    ${method}    ${url}
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}    local=true
    Check Response Status Code    201    ${response.status_code}

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
    ...    entity_id=${entity_id}
    ...    mode=exclusive
    ...    endpoint=/broker1
    ...    operations=${operations}
    ${response1}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response1.status_code}
    Start Context Source Mock Server

    ${serialized_entity}=    Load Entity As Serialized Array    ${entity_speed_filename}    ${entity_id}
    @{entities_id}=    Create List    ${entity_id}

    IF    '${method}' == 'GET'
        Set Stub Reply    ${method}    ${url}?type=Vehicle&id=${entity_id}&attrs=speed    200    ${serialized_entity}
        ${response2}=    Query Entities    entity_types=Vehicle    context=${ngsild_test_suite_context}
    ELSE IF    '${method}' == 'POST'
        Set Stub Reply    ${method}    ${url}    200    ${serialized_entity}
        ${entity_selector}=    Create Dictionary    type=Vehicle    id=${entity_id}
        @{entities}=    Create List    ${entity_selector}
        ${response2}=    Query Entities Via POST    entities=${entities}    context=${ngsild_test_suite_context}
    END

    Check Response Status Code    200    ${response2.status_code}
    Check Response Body Containing Entities URIS set to    ${entities_id}    ${response2.json()}
    Should Have Value In Json    ${response2.json()[0]}    $.speed

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${entity_id}
    Stop Context Source Mock Server

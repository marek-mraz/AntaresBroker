*** Settings ***
Documentation       Verify that when an entity remotely exists on two Context Sources, a retrieval request to the Context Broker is correctly forwarded to the Context Sources and the response contains the identity.

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Setup Registration And Start Context Source Mock Server
Test Teardown       Delete Created Entity And Registration And Stop Context Source Mock Server


*** Variables ***
${entity_id_prefix}                     urn:ngsi-ld:Vehicle:
${entity_payload_filename}              vehicle-simple-attributes.json
${entity_payload_filename2}             vehicle-simple-attributes-second.json
${registration_id_prefix}               urn:ngsi-ld:Registration:
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-redirection-ops.jsonld


*** Test Cases ***
D010_01_red Query Context Broker And Retrieve Entity By Id
    [Documentation]    Check that if one retrieves an entity on the Context Source, entity gets redirected correctly
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-redirect    4_3_6_3    5_7_1
    ${entity_body}=    Load Entity    ${entity_payload_filename}    ${entity_id}
    ${entity_body2}=    Load Entity    ${entity_payload_filename2}    ${entity_id}
    Set Stub Reply    GET    /broker1/ngsi-ld/v1/entities/${entity_id}    200    ${entity_body}
    Set Stub Reply    GET    /broker2/ngsi-ld/v1/entities/${entity_id}    200    ${entity_body2}

    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}

    ${stub_count}=    Get Stub Count    GET    /broker1/ngsi-ld/v1/entities/${entity_id}
    Should Be True    ${stub_count} > 0
    ${stub_count}=    Get Stub Count    GET    /broker2/ngsi-ld/v1/entities/${entity_id}
    Should Be True    ${stub_count} > 0

    Should Have Value In Json    ${response.json()}    $.isParked
    Should Have Value In Json    ${response.json()}    $.isParked2


*** Keywords ***
Setup Registration And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    mode=redirect
    ...    endpoint=/broker1
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id2}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id2}
    ${registration_payload2}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    mode=redirect
    ...    endpoint=/broker2
    ${response}=    Create Context Source Registration With Return    ${registration_payload2}
    Check Response Status Code    201    ${response.status_code}

    Start Context Source Mock Server

Delete Created Entity And Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Context Source Registration    ${registration_id2}
    Delete Entity    ${entity_id}
    Stop Context Source Mock Server

*** Settings ***
Documentation       Verify that, when one has an exclusive registration on a Context Broker, one is able to purge entities based on type on both Context Broker and Context Source

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Registrations And Stop Context Source Mock Server


*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${speed_entity_filename}                vehicle-speed-attribute.jsonld
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld


*** Test Cases ***
D017_01_exc Purge Entities On Both Context Broker And Context Source
    [Documentation]    Verify that, when one has an exclusive registration on a Context Broker, one is able to purge entities based on type on both Context Broker and Context Source
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-exclusive    4_3_6_3    5_6_21

    Set Stub Reply    POST    /broker1/ngsi-ld/v1/entities    201
    ${speed_entity}=    Create Entity    ${speed_entity_filename}    ${entity_id}

    Set Stub Reply    DELETE    /broker1/ngsi-ld/v1/entities?type=Vehicle    204
    ${response}=    Purge Entities    type=Vehicle    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}

    ${stub_count}=    Get Stub Count    DELETE    /broker1/ngsi-ld/v1/entities?type=Vehicle
    Should Be True    ${stub_count} > 0

    ${response}=    Retrieve Entity    ${entity_id}
    Check Response Status Code    404    ${response.status_code}


*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}
    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}    local=true
    Check Response Status Code    201    ${response.status_code}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    endpoint=/broker1
    ...    mode=exclusive
    ${response1}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response1.status_code}
    Start Context Source Mock Server

Delete Registrations And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server

*** Settings ***
Documentation       Check that if one request the Context Broker to delete an attribute with the deleteAll flag, all the instances of that attribute are deleted in the Context Source.
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource


Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Registration And Stop Context Source Mock Server

*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld
${attribute_id}                         speed

*** Test Cases ***
D006_02_exc Delete Entity Attributes
    [Documentation]    Verify that, when one has an exclusive registration on a Context Broker with redirectionOps, one is able to delete entities attributes on a Context Source
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-exclusive    4_3_6_3    5_6_5

    Set Stub Reply    DELETE    /broker1/ngsi-ld/v1/entities/${entity_id}/attrs/${attribute_id}    204
    ${response}=    Delete Entity Attributes
    ...    ${entity_id}
    ...    ${attribute_id}
    ...    ${EMPTY}
    ...    true
    ...    ${ngsild_test_suite_context}
    Wait For Request
    Check Response Status Code    204    ${response.status_code}

    ${stub_count}=    Get Stub Count    DELETE    /broker1/ngsi-ld/v1/entities/${entity_id}/attrs/${attribute_id}
    Should Be True    ${stub_count} > 0

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
    ...    entity_id=${entity_id}
    ...    mode=exclusive
    ...    endpoint=/broker1
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}
    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${entity_id}
    Stop Context Source Mock Server
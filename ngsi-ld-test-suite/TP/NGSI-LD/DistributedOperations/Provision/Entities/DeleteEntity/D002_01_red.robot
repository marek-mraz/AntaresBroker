*** Settings ***
Documentation       Verify that when two redirect registrations are configured on a Context Broker, it is possible to delete entities in both Context Sources

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup         Setup Entity Id And Registration And Start Context Source Mock Server
Test Teardown      Delete Registrations And Stop Context Source Mock Server


*** Variables ***
${entity_id_prefix}                     urn:ngsi-ld:Vehicle:
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${registration_id_prefix}               urn:ngsi-ld:Registration:
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-redirection-ops.jsonld


*** Test Cases ***
D002_01_red Delete Entities On Both Context Sources
    [Documentation]    Verify that, when one has a redirect registration on a Context Broker, one is able to delete entities on both Context Sources
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxied-redirect    4_3_6_3    5_6_6

    Set Stub Reply  POST    /broker1/ngsi-ld/v1/entities    201
    Set Stub Reply  POST    /broker2/ngsi-ld/v1/entities    201

    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id2}
    Check Response Status Code    201    ${response.status_code}

    Set Stub Reply  DELETE    /broker1/ngsi-ld/v1/entities/${entity_id}    204
    Set Stub Reply  DELETE    /broker2/ngsi-ld/v1/entities/${entity_id2}    204
    
    ${response}=    Delete Entity    ${entity_id}
    Check Response Status Code    204    ${response.status_code}  
    ${response}=    Delete Entity    ${entity_id2}
    Check Response Status Code    204    ${response.status_code}
    
    ${stub_count}=    Get Stub Count    DELETE    /broker1/ngsi-ld/v1/entities/${entity_id}
    Should Be True    ${stub_count} > 0
    ${stub_count}=    Get Stub Count    DELETE    /broker2/ngsi-ld/v1/entities/${entity_id2}
    Should Be True    ${stub_count} > 0

*** Keywords ***
Setup Entity Id And Registration And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}
    ${entity_id2}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id2}

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
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id2}
    ...    mode=redirect
    ...    endpoint=/broker2
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    Start Context Source Mock Server

Delete Registrations And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Context Source Registration    ${registration_id2}
    Stop Context Source Mock Server
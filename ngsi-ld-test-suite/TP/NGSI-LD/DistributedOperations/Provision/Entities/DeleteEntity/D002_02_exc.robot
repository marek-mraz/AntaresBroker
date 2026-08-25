*** Settings ***
Documentation       Verify that an entity cannot be deleted on the Context Source if the registration does not support the correct operations.

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Created Entity And Registration And Stop Context Source Mock Server


*** Variables ***
${entity_id_prefix}                     urn:ngsi-ld:Vehicle:
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${registration_id_prefix}               urn:ngsi-ld:Registration:
# ETSI tool bug fixed: 4.3.6.3 requires an exclusive registration to define Attributes;
# the attribute-less vehicle-redirection-ops fixture cannot legally be exclusive.
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld


*** Test Cases ***
D002_02_exc Delete Entity Without Redirection Operations
    [Documentation]    Check that the deletion is not forwarded to the Context Source due to a lack of redirection operations in the registration.
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-exclusive    4_3_6_3    5_6_6

    Set Stub Reply    DELETE    /broker1/ngsi-ld/v1/entities/${entity_id}    207
    ${response}=    Delete Entity    ${entity_id}
    Check Response Status Code    207    ${response.status_code}

    ${stub_count}=    Get Stub Count    DELETE    /broker1/ngsi-ld/v1/entities/${entity_id}
    Should Be True    ${stub_count} == 0


*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}    local=true
    Check Response Status Code    201    ${response.status_code}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    # ETSI tool bug fixed: the file's operations=["redirectionOps"] INCLUDES deleteEntity
    # (Table 4.20-2), so the broker was spec-required (5.6.6.4) to forward - contradicting
    # this test's premise ("lack of redirection operations"). Override with an operations
    # list lacking deleteEntity: per 5.6.6.4 the unsupported exclusive part -> Conflict,
    # while "the input data shall be used to remove the entity locally if it exists"
    # succeeds -> partial success -> 207 as this test expects.
    ${operations}=    Create List    queryEntity
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    mode=exclusive
    ...    endpoint=/broker1
    ...    operations=${operations}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    Start Context Source Mock Server

Delete Created Entity And Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${entity_id}
    Stop Context Source Mock Server
